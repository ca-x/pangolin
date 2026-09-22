import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider, useProject } from '../project'
import PlaygroundPage from './PlaygroundPage'

/**
 * The playground's model picker is the key's own `/v1/models` list. When that
 * lookup failed the control fell back to an empty list, which the page rendered
 * as "this key reaches no models" — a claim it had no evidence for. The failure
 * now says what it is and offers the retry.
 */
const json = (value: unknown, status = 200) => Promise.resolve(new Response(JSON.stringify(value), { status, headers: { 'Content-Type': 'application/json' } }))
const project = { id: 'p1', name: 'Project', slug: 'project', owner_user_id: 'u1', is_default: true, enabled: true }
const projectTwo = { ...project, id: 'p2', name: 'Project Two', slug: 'project-two', is_default: false }

const key = (id: string, name = id) => ({ id, name, enabled: true, expires_at: null, budget_micros: null, spent_micros: 0 })
const mockApi = (answer: (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>, projects = [project], keys = [key('key-1')]) => vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
  const path = String(input)
  if (path.includes('/api-keys')) return json(keys)
  if (path.includes('/playground/models')) return answer(input, init)
  if (path.includes('/operations/channels')) return json({ data: [] })
  if (path.endsWith('/projects')) return json(projects)
  if (path.includes('/permissions')) return json(['*'])
  if (path.includes('/playground/chat')) return json({ status: 'completed', output_text: 'ok' })
  return json({})
})

function ProjectSwitch() {
  const { projects, setProjectId } = useProject()
  return <>{projects.map((candidate) => <button type="button" key={candidate.id} onClick={() => setProjectId(candidate.id)}>{candidate.name}</button>)}</>
}

const renderPage = (withProjectSwitch = false) => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider>{withProjectSwitch && <ProjectSwitch />}<PlaygroundPage /></ProjectProvider></MemoryRouter></QueryClientProvider>)
}

const deferred = <T,>() => {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => { resolve = done })
  return { promise, resolve }
}

describe('the playground model picker', () => {
  beforeEach(() => { void i18n.changeLanguage('en') })

  it('reports a failed model lookup instead of claiming the key reaches nothing', async () => {
    const fetchMock = mockApi(() => Promise.reject(new Error('models unavailable')))
    vi.stubGlobal('fetch', fetchMock)
    renderPage()

    // The model request starts only after the project and key queries settle.
    // Under the full-suite worker load that chain can legitimately exceed the
    // Testing Library one-second default without changing the rendered result.
    expect(await screen.findByText('The option list could not be loaded.', {}, { timeout: 3_000 })).toBeInTheDocument()
    expect(screen.queryByRole('combobox', { name: 'Model' })).not.toBeInTheDocument()
    expect(screen.queryByText('No reachable models')).not.toBeInTheDocument()

    const before = fetchMock.mock.calls.length
    await userEvent.click(screen.getByRole('button', { name: 'Retry' }))
    await vi.waitFor(() => expect(fetchMock.mock.calls.length).toBeGreaterThan(before))
  })

  it('keeps the picker when the lookup succeeds', async () => {
    vi.stubGlobal('fetch', mockApi(() => json({ data: [{ id: 'gpt-4o' }] })))
    renderPage()

    expect(await screen.findByRole('combobox', { name: 'Model' })).toBeInTheDocument()
    expect(screen.queryByText('The option list could not be loaded.')).not.toBeInTheDocument()
  })

  it('accepts the release API string model shape without crashing the picker', async () => {
    vi.stubGlobal('fetch', mockApi(() => json({ data: ['qa-model'] })))
    renderPage()

    const picker = await screen.findByRole('combobox', { name: 'Model' })
    await userEvent.click(picker)
    expect(await screen.findByRole('option', { name: 'qa-model' })).toBeInTheDocument()
  })

  it('asks the server for models reachable through the Responses endpoint', async () => {
    const fetchMock = mockApi(() => json({ data: [{ id: 'responses-model' }] }))
    vi.stubGlobal('fetch', fetchMock)
    renderPage()

    await screen.findByRole('combobox', { name: 'Model' })
    await vi.waitFor(() => expect(fetchMock.mock.calls.some(([input]) => String(input).includes('/playground/models'))).toBe(true))
    const request = fetchMock.mock.calls.find(([input]) => String(input).includes('/playground/models'))
    expect(request).toBeDefined()
    const url = new URL(String(request?.[0]), 'https://pangolin.test')
    expect(url.pathname).toBe('/api/admin/v1/projects/p1/playground/models')
    expect(url.searchParams.get('api_key_id')).toBe('key-1')
  })

  it('explains an endpoint-filtered empty list and lets the operator retry', async () => {
    const fetchMock = mockApi(() => json({ data: [] }))
    vi.stubGlobal('fetch', fetchMock)
    renderPage()

    await vi.waitFor(() => expect(fetchMock.mock.calls.some(([input]) => String(input).includes('/playground/models'))).toBe(true))
    expect((await screen.findAllByText('This key and its policies cannot reach any model through the Responses API.')).length).toBeGreaterThan(0)
    expect(screen.getByRole('combobox', { name: 'Model' })).toBeDisabled()

    const before = fetchMock.mock.calls.filter(([input]) => String(input).includes('/playground/models')).length
    await userEvent.click(screen.getByRole('button', { name: 'Retry' }))
    await vi.waitFor(() => {
      const after = fetchMock.mock.calls.filter(([input]) => String(input).includes('/playground/models')).length
      expect(after).toBeGreaterThan(before)
    })
  })

  it('sends the model actually chosen from the endpoint-filtered list', async () => {
    const fetchMock = mockApi(() => json({ data: [{ id: 'responses-a' }, { id: 'responses-b' }] }))
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    const user = userEvent.setup()

    await user.click(await screen.findByRole('combobox', { name: 'Model' }))
    await user.click(await screen.findByRole('option', { name: 'responses-b' }))
    await user.type(screen.getByLabelText(/^Input/), 'hello')
    await user.click(screen.getByRole('button', { name: 'Send request' }))

    const request = fetchMock.mock.calls.find(([input]) => String(input).includes('/playground/chat'))
    expect(request).toBeDefined()
    const body = JSON.parse(String((request?.[1] as RequestInit).body))
    expect(body.api_key_id).toBe('key-1')
    expect(body.payload.model).toBe('responses-b')
    expect(new Headers((request?.[1] as RequestInit).headers).get('authorization')).toBeNull()
  })

  it('does not let an older key lookup restore a stale option or selection', async () => {
    const first = deferred<Response>()
    const fetchMock = mockApi((input) => {
      const selected = new URL(String(input), 'https://pangolin.test').searchParams.get('api_key_id')
      return selected === 'key-first'
        ? first.promise
        : json({ data: [{ id: 'current-model' }] })
    }, [project], [key('key-first'), key('key-second')])
    vi.stubGlobal('fetch', fetchMock)
    renderPage()

    const keyPicker = await screen.findByRole('combobox', { name: 'API key' })
    await userEvent.click(keyPicker)
    await userEvent.click(await screen.findByRole('option', { name: 'key-second' }))
    const picker = await screen.findByRole('combobox', { name: 'Model' })
    await userEvent.click(picker)
    await userEvent.click(await screen.findByRole('option', { name: 'current-model' }))

    await act(async () => { first.resolve(await json({ data: [{ id: 'stale-model' }] })) })
    expect(screen.getByRole('combobox', { name: 'Model' })).toHaveValue('current-model')
    await userEvent.click(screen.getByRole('combobox', { name: 'Model' }))
    expect(screen.queryByRole('option', { name: 'stale-model' })).not.toBeInTheDocument()
  })

  it('clears the selected model when the active project changes', async () => {
    let modelRequest = 0
    const fetchMock = mockApi(() => {
      modelRequest += 1
      return json({ data: [{ id: modelRequest === 1 ? 'project-one-model' : 'project-two-model' }] })
    }, [project, projectTwo])
    vi.stubGlobal('fetch', fetchMock)
    renderPage(true)

    const user = userEvent.setup()
    await user.click(await screen.findByRole('combobox', { name: 'Model' }))
    await user.click(await screen.findByRole('option', { name: 'project-one-model' }))
    expect(screen.getByRole('combobox', { name: 'Model' })).toHaveValue('project-one-model')

    await user.click(screen.getByRole('button', { name: 'Project Two' }))
    await vi.waitFor(() => expect(screen.getByRole('combobox', { name: 'Model' })).toHaveValue(''))
    await user.click(screen.getByRole('combobox', { name: 'Model' }))
    expect(await screen.findByRole('option', { name: 'project-two-model' })).toBeInTheDocument()
    expect(screen.queryByRole('option', { name: 'project-one-model' })).not.toBeInTheDocument()
  })
})
