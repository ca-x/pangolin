import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import PlaygroundPage from './PlaygroundPage'

/**
 * The playground only ever spoke SSE: it posted `/v1/responses` with
 * `stream: true` whatever the selected model or upstream could do, so an
 * upstream that answers a single JSON body was unreachable from the console.
 * And when the request failed the response panel showed the error alert *and*
 * the idle "send a request" copy at once, which reads as two different states
 * of the same panel.
 *
 * The streamed fixtures are the shapes the gateway writes: `stream::encode`
 * frames an `event:` line and the same event name inside `data:`, and a
 * `/v1/responses` stream ends with a terminal event.
 */
const json = (value: unknown, status = 200) => Promise.resolve(new Response(JSON.stringify(value), { status, headers: { 'Content-Type': 'application/json' } }))
const project = { id: 'p1', name: 'Project', slug: 'project', owner_user_id: 'u1', is_default: true, enabled: true }
const models = { data: [{ id: 'qa-fast' }] }
const record = { executions: [], usage: [], cost_items: [] }

const frame = (name: string, payload: unknown) => `event: ${name}\ndata: ${JSON.stringify(payload)}\n\n`
const delta = (text: string) => frame('response.output_text.delta', { type: 'response.output_text.delta', delta: text })
const completed = frame('response.completed', { type: 'response.completed', response: { id: 'resp_1', status: 'completed', output: [] } })
const sse = (body: string) => Promise.resolve(new Response(body, { status: 200, headers: { 'Content-Type': 'text/event-stream', 'x-request-id': 'req_c2' } }))

const mockApi = (answer: () => Promise<Response>) => vi.fn((input: RequestInfo | URL) => {
  const path = String(input)
  if (path.includes('/api-keys')) return json([{ id: 'key-1', name: 'pk_test_123456', enabled: true, expires_at: null, budget_micros: null, spent_micros: 0 }])
  if (path.includes('/playground/models')) return json(models)
  if (path.includes('/playground/chat')) return answer()
  if (path.includes('/operations/requests/')) return json(record)
  if (path.endsWith('/projects')) return json([project])
  if (path.includes('/permissions')) return json(['*'])
  return json({})
})

const renderPage = () => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><PlaygroundPage /></ProjectProvider></MemoryRouter></QueryClientProvider>)
}

const fillForm = async (user: ReturnType<typeof userEvent.setup>) => {
  await user.click(await screen.findByRole('combobox', { name: 'Model' }))
  await user.click(await screen.findByRole('option', { name: 'qa-fast' }))
  await user.type(screen.getByLabelText(/^Input/), 'hello')
}

const body = (fetchMock: ReturnType<typeof vi.fn>) => JSON.parse(String((fetchMock.mock.calls.find(([input]) => String(input).includes('/playground/chat'))?.[1] as RequestInit).body)).payload

describe('the playground can reach an upstream that does not stream', () => {
  beforeEach(() => { void i18n.changeLanguage('en') })

  it('sends stream: false and shows the body it got back', async () => {
    const user = userEvent.setup()
    const fetchMock = mockApi(() => json({ output_text: 'a single JSON answer' }))
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    await fillForm(user)
    await user.click(screen.getByRole('switch', { name: /Stream response/ }))
    await user.click(screen.getByRole('button', { name: 'Send request' }))

    expect((await screen.findAllByText('a single JSON answer')).length).toBeGreaterThan(0)
    expect(body(fetchMock).stream).toBe(false)
    // The idle copy is a state of the panel, not decoration under an answer.
    expect(screen.queryByText('Send a request to inspect the raw response here.')).not.toBeInTheDocument()
  })

  it('still streams by default', async () => {
    const user = userEvent.setup()
    const fetchMock = mockApi(() => sse(delta('streamed') + completed))
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    await fillForm(user)
    await user.click(screen.getByRole('button', { name: 'Send request' }))

    expect((await screen.findAllByText('streamed')).length).toBeGreaterThan(0)
    expect(body(fetchMock).stream).toBe(true)
    expect(await screen.findByText('Outcome: Completed')).toBeInTheDocument()
  })

  it('shows the failure alone, never together with the idle copy', async () => {
    const user = userEvent.setup()
    vi.stubGlobal('fetch', mockApi(() => json({ error: { type: 'upstream_error', message: 'all eligible upstream attempts failed' } }, 502)))
    renderPage()
    await fillForm(user)
    await user.click(screen.getByRole('button', { name: 'Send request' }))

    expect(await screen.findByText('all eligible upstream attempts failed')).toBeInTheDocument()
    expect(screen.queryByText('Send a request to inspect the raw response here.')).not.toBeInTheDocument()
  })

  it('reports a completed stream that carried no text instead of claiming nothing was sent', async () => {
    const user = userEvent.setup()
    vi.stubGlobal('fetch', mockApi(() => sse(completed)))
    renderPage()
    await fillForm(user)
    await user.click(screen.getByRole('button', { name: 'Send request' }))

    expect(await screen.findByText(/no text output/i)).toBeInTheDocument()
    expect(screen.queryByText('Send a request to inspect the raw response here.')).not.toBeInTheDocument()
  })

  it('does not report an empty stream that ended without a terminal event as completed', async () => {
    const user = userEvent.setup()
    vi.stubGlobal('fetch', mockApi(() => sse('')))
    renderPage()
    await fillForm(user)
    await user.click(screen.getByRole('button', { name: 'Send request' }))

    expect(await screen.findByText('Outcome: Interrupted')).toBeInTheDocument()
    expect(screen.queryByText('Outcome: Completed')).not.toBeInTheDocument()
    expect(screen.queryByText('Send a request to inspect the raw response here.')).not.toBeInTheDocument()
  })
})
