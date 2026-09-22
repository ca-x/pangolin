import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import PlaygroundPage from './PlaygroundPage'

const json = (value: unknown, headers: Record<string, string> = {}) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json', ...headers } }))
const project = { id: 'p1', name: 'Project', slug: 'project', owner_user_id: 'u1', is_default: true, enabled: true }
const bootstrap = { initialized: true, authenticated: true, user: { id: 'u1', email: 'owner@example.test', role: 'admin', language: 'en', theme: 'system:bronze', created_at: 1 }, capture_payloads: false, observability_available: true, branding: { instance_name: 'Pangolin', branding_name: 'Pangolin', favicon_url: '/logo.webp', onboarding_complete: true } }
const usableKey = { id: 'key-internal', name: 'Console key', enabled: true, expires_at: null, budget_micros: null, spent_micros: 0 }

const renderPage = (fetchMock: ReturnType<typeof vi.fn>) => {
  vi.stubGlobal('fetch', fetchMock)
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><PlaygroundPage /></ProjectProvider></MemoryRouter></QueryClientProvider>)
}

const mockApi = () => {
  let response = 0
  return vi.fn((input: RequestInfo | URL, _init?: RequestInit) => {
    const path = String(input)
    if (path.includes('/api/v1/bootstrap')) return json(bootstrap)
    if (path.endsWith('/projects')) return json([project])
    if (path.includes('/permissions')) return json(['*'])
    if (path.includes('/api-keys')) return json([usableKey])
    if (path.includes('/operations/channels')) return json({ data: [{ id: 'provider-1', name: 'Channel one', enabled: true }] })
    if (path.includes('/playground/models')) return json({ data: [{ id: 'responses-model' }] })
    if (path.includes('/playground/chat')) {
      response += 1
      return json({ status: 'completed', output_text: response === 1 ? 'first answer' : 'second answer' }, { 'x-request-id': `request-${response}` })
    }
    if (path.includes('/operations/requests/')) return json({ id: `internal-${response}`, usage: [] })
    return json({})
  })
}

async function ready(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByRole('combobox', { name: 'Model' }))
  await user.click(await screen.findByRole('option', { name: 'responses-model' }))
  await user.click(screen.getByRole('switch', { name: /Stream response/ }))
}

describe('the session-authenticated playground', () => {
  beforeEach(async () => { await i18n.changeLanguage('en') })

  it('uses an internal project key without putting its token in the browser request', async () => {
    const fetchMock = mockApi()
    renderPage(fetchMock)
    const user = userEvent.setup()
    await ready(user)
    await user.type(screen.getByLabelText(/^Input/), 'hello')
    await user.click(screen.getByRole('button', { name: 'Send request' }))

    expect(await screen.findByText('first answer')).toBeInTheDocument()
    const call = fetchMock.mock.calls.find(([input]) => String(input).includes('/playground/chat'))!
    const init = call[1] as RequestInit
    const body = JSON.parse(String(init.body))
    expect(body.api_key_id).toBe('key-internal')
    expect(body.payload.input).toEqual([{ role: 'user', content: 'hello' }])
    expect(new Headers(init.headers).get('authorization')).toBeNull()
    expect(new Headers(init.headers).get('x-pangolin-csrf')).toBe('1')
  })

  it('sends ordered multi-turn history and can clear it', async () => {
    const fetchMock = mockApi()
    renderPage(fetchMock)
    const user = userEvent.setup()
    await ready(user)
    await user.type(screen.getByLabelText(/^Input/), 'first')
    await user.click(screen.getByRole('button', { name: 'Send request' }))
    await screen.findByText('first answer')
    await user.type(screen.getByLabelText(/^Input/), 'second')
    await user.click(screen.getByRole('button', { name: 'Send request' }))
    await screen.findByText('second answer')

    const calls = fetchMock.mock.calls.filter(([input]) => String(input).includes('/playground/chat'))
    const second = JSON.parse(String((calls[1][1] as RequestInit).body))
    expect(second.payload.input).toEqual([
      { role: 'user', content: 'first' },
      { role: 'assistant', content: 'first answer' },
      { role: 'user', content: 'second' },
    ])
    expect(within(screen.getByRole('log', { name: 'Playground conversation' })).getByText('second answer')).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: 'Clear conversation' }))
    expect(screen.queryByRole('log', { name: 'Playground conversation' })).not.toBeInTheDocument()
  })

  it('sends a chosen channel as a routing constraint', async () => {
    const fetchMock = mockApi()
    renderPage(fetchMock)
    const user = userEvent.setup()
    await ready(user)
    await user.click(screen.getByRole('combobox', { name: 'Request source' }))
    await user.click(await screen.findByRole('option', { name: 'Channel one' }))
    await user.type(screen.getByLabelText(/^Input/), 'route this')
    await user.click(screen.getByRole('button', { name: 'Send request' }))
    await screen.findByText('first answer')

    const call = fetchMock.mock.calls.find(([input]) => String(input).includes('/playground/chat'))!
    expect(JSON.parse(String((call[1] as RequestInit).body)).provider_id).toBe('provider-1')
  })
})
