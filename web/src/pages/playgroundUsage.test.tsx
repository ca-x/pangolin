import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import PlaygroundPage from './PlaygroundPage'

/**
 * A playground run wrote 11 input tokens, 7 output tokens and a 14 µUSD charge to
 * the record system, and the console showed none of it: the response header names
 * the request (`x-request-id`) and the project-scoped record route returns its
 * `usage[]`, but nothing read it. The console has to show what the record holds —
 * and `—` where the record holds nothing, never an invented zero.
 *
 * The mock is the shape `/api/admin/v1/projects/{project}/operations/requests/{id}`
 * returns: the request row plus `executions`, `usage` and `cost_items`
 * (`src/api/operations_api.rs`). That read is the compatibility bridge: it is asked for
 * the repeatable header id and answers with the record's own `id`, which is the identity
 * the request-detail page opens. The panel's "View request" link therefore uses the
 * record's `id`, never the header id — two requests can share the header id — and no link
 * is offered while the internal id is unknown (still loading, or the read failed).
 */
const json = (value: unknown, status = 200) => Promise.resolve(new Response(JSON.stringify(value), { status, headers: { 'Content-Type': 'application/json' } }))
const project = { id: 'p1', name: 'Project', slug: 'project', owner_user_id: 'u1', is_default: true, enabled: true }
const models = { data: [{ id: 'qa-fast' }] }
/** Two settled attempts, summing to 11 input tokens, 7 output tokens and 14 µUSD. */
const record = {
  id: 'internal-1',
  public_id: 'req_c2',
  status: 'succeeded',
  executions: [
    { id: 'e1', request_id: 'internal-1', provider_id: 'openai', attempt: 1, model: 'demo', status: 'failed', retry_reason: 'upstream_5xx', latency_ms: 5 },
    { id: 'e2', request_id: 'internal-1', provider_id: 'backup', attempt: 2, model: 'demo', status: 'succeeded', retry_reason: null, latency_ms: 7 },
  ],
  usage: [
    { execution_id: 'e1', model_id: 'm1', input_tokens: 4, output_tokens: 3, cache_read_tokens: 0, cache_write_tokens: 0, reasoning_tokens: 0, total_cost_micros: 6, created_at: 1 },
    { execution_id: 'e2', model_id: 'm1', input_tokens: 7, output_tokens: 4, cache_read_tokens: 0, cache_write_tokens: 0, reasoning_tokens: 0, total_cost_micros: 8, created_at: 2 },
  ],
  cost_items: [{ execution_id: 'e2', kind: 'input', quantity: 7, unit_price_micros: 1, subtotal_micros: 7 }],
}

const frame = (name: string, payload: unknown) => `event: ${name}\ndata: ${JSON.stringify(payload)}\n\n`
const delta = (text: string) => frame('response.output_text.delta', { type: 'response.output_text.delta', delta: text })
const completed = frame('response.completed', { type: 'response.completed', response: { id: 'resp_1', status: 'completed', output: [] } })
const sse = (frames: string[]) => new Response(frames.join(''), { status: 200, headers: { 'Content-Type': 'text/event-stream', 'x-request-id': 'req_c2' } })
const document = (status = 'completed') => Promise.resolve(new Response(JSON.stringify({ id: 'resp_1', object: 'response', status, output: [{ type: 'message', role: 'assistant', content: [{ type: 'output_text', text: 'a single JSON answer' }] }] }), { status: 200, headers: { 'Content-Type': 'application/json', 'x-request-id': 'req_c2' } }))

const mockApi = (answer: () => Promise<Response>, request: () => Promise<Response>) => vi.fn((input: RequestInfo | URL) => {
  const path = String(input)
  if (path.includes('/api-keys')) return json([{ id: 'key-1', name: 'pk_test_123456', enabled: true, expires_at: null, budget_micros: null, spent_micros: 0 }])
  if (path.includes('/playground/models')) return json(models)
  if (path.includes('/playground/chat')) return answer()
  if (path.includes('/operations/requests/')) return request()
  if (path.endsWith('/projects')) return json([project])
  if (path.includes('/permissions')) return json(['*'])
  return json({})
})

const renderPage = () => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><PlaygroundPage /></ProjectProvider></MemoryRouter></QueryClientProvider>)
  return client
}

const fillForm = async (user: ReturnType<typeof userEvent.setup>) => {
  await user.click(await screen.findByRole('combobox', { name: 'Model' }))
  await user.click(await screen.findByRole('option', { name: 'qa-fast' }))
  await user.type(screen.getByLabelText(/^Input/), 'hello')
}

const send = async (fetchMock: ReturnType<typeof vi.fn>, streaming = true) => {
  const user = userEvent.setup()
  vi.stubGlobal('fetch', fetchMock)
  const client = renderPage()
  await fillForm(user)
  if (!streaming) await user.click(screen.getByRole('switch', { name: /Stream response/ }))
  await user.click(screen.getByRole('button', { name: 'Send request' }))
  return { user, client }
}

const recordCalls = (fetchMock: ReturnType<typeof vi.fn>) => fetchMock.mock.calls.filter(([input]) => String(input).includes('/operations/requests/'))

describe('the playground shows the usage and cost the record holds', () => {
  beforeEach(() => { void i18n.changeLanguage('en') })

  it('sums the usage rows of the request the gateway identified', async () => {
    const fetchMock = mockApi(() => Promise.resolve(sse([delta('streamed answer'), completed])), () => json(record))
    const { client } = await send(fetchMock)

    expect(await screen.findByText('streamed answer')).toBeInTheDocument()
    expect(await screen.findByText('Outcome: Completed')).toBeInTheDocument()
    expect(await screen.findByText('Input tokens: 11')).toBeInTheDocument()
    expect(await screen.findByText('Output tokens: 7')).toBeInTheDocument()
    expect(await screen.findByText('Cost: $0.000014')).toBeInTheDocument()
    // The header id is the caller's and a caller may repeat it, so it is not the
    // navigation target: the record's own `id` is. Both identities stay on the panel —
    // the external one is what the operator correlates with the client.
    expect(screen.getByText('req_c2')).toBeInTheDocument()
    expect(screen.getByRole('link', { name: 'View request' })).toHaveAttribute('href', '/operations/requests/internal-1')

    const calls = recordCalls(fetchMock)
    expect(calls).toHaveLength(1)
    // The lookup still starts from the header id: that is the compatibility bridge the
    // backend resolves into the record, and the only way to discover the internal id.
    expect(String(calls[0][0])).toBe('/api/admin/v1/projects/p1/operations/requests/req_c2')
    // The key lives in the form and nowhere else: not in the record read, and not
    // in any cache key the console holds.
    expect(JSON.stringify(calls[0][1] ?? {})).not.toContain('pk_live_123456')
    const keys = JSON.stringify(client.getQueryCache().getAll().map((query) => query.queryKey))
    expect(keys).toContain('req_c2')
    expect(keys).toContain('p1')
    expect(keys).not.toContain('pk_live_123456')
  })

  it('shows the same facts for a non-streaming response', async () => {
    const fetchMock = mockApi(() => document(), () => json(record))
    await send(fetchMock, false)

    expect(await screen.findByText('a single JSON answer')).toBeInTheDocument()
    expect(await screen.findByText('Input tokens: 11')).toBeInTheDocument()
    expect(await screen.findByText('Output tokens: 7')).toBeInTheDocument()
    expect(await screen.findByText('Cost: $0.000014')).toBeInTheDocument()
    expect(recordCalls(fetchMock)).toHaveLength(1)
  })

  it('shows only the public error of a 200 document that reports its own failure', async () => {
    // The gateway can pass an upstream envelope through verbatim, so the document
    // may carry a nested cause. Rendering the document — which is what an answer
    // with no text falls back to — would print it, and calling a 200 a success
    // would be the same lie in the other direction.
    const failed = { id: 'resp_1', object: 'response', status: 'failed', error: { type: 'upstream_error', message: 'upstream request failed', cause: { detail: 'private-upstream-secret' } } }
    const fetchMock = mockApi(() => Promise.resolve(new Response(JSON.stringify(failed), { status: 200, headers: { 'Content-Type': 'application/json', 'x-request-id': 'req_c2' } })), () => json(record))
    await send(fetchMock, false)

    expect(await screen.findByText('Outcome: Failed')).toBeInTheDocument()
    expect(await screen.findByText('Failed · upstream_error')).toBeInTheDocument()
    expect(await screen.findByText('upstream request failed')).toBeInTheDocument()
    expect(screen.queryByText(/private-upstream-secret/)).not.toBeInTheDocument()
    expect(screen.queryByText('Outcome: Completed')).not.toBeInTheDocument()
  })

  it('uses the localized fallback for a failed document that carries no public message', async () => {
    const failed = { id: 'resp_1', object: 'response', status: 'incomplete' }
    const fetchMock = mockApi(() => Promise.resolve(new Response(JSON.stringify(failed), { status: 200, headers: { 'Content-Type': 'application/json', 'x-request-id': 'req_c2' } })), () => json(record))
    await send(fetchMock, false)

    expect(await screen.findByText('Outcome: Incomplete')).toBeInTheDocument()
    expect(await screen.findByText(/The response is incomplete/)).toBeInTheDocument()
    // A document that reports its own failure is not an answer: it is not rendered.
    expect(screen.queryByText(/"status": "incomplete"/)).not.toBeInTheDocument()
    expect(screen.queryByText('Outcome: Completed')).not.toBeInTheDocument()
  })

  it('prints a measured zero as a zero, and an unmeasured fact as —', async () => {
    const fetchMock = mockApi(() => Promise.resolve(sse([delta('nothing was measured'), completed])), () => json({ ...record, usage: [{ execution_id: 'e1', input_tokens: 0, output_tokens: 0, total_cost_micros: 0 }] }))
    await send(fetchMock)

    expect(await screen.findByText('Input tokens: 0')).toBeInTheDocument()
    expect(await screen.findByText('Output tokens: 0')).toBeInTheDocument()
    expect(await screen.findByText('Cost: $0.000000')).toBeInTheDocument()
  })

  it('does not invent a zero for a record that holds no usage at all', async () => {
    const fetchMock = mockApi(() => Promise.resolve(sse([delta('no usage rows'), completed])), () => json({ ...record, usage: [] }))
    await send(fetchMock)

    expect(await screen.findByText('Input tokens: —')).toBeInTheDocument()
    expect(await screen.findByText('Output tokens: —')).toBeInTheDocument()
    expect(await screen.findByText('Cost: —')).toBeInTheDocument()
    expect(screen.queryByText('Input tokens: 0')).not.toBeInTheDocument()
  })

  it('keeps the response visible while the record is still being read', async () => {
    const fetchMock = mockApi(() => Promise.resolve(sse([delta('the answer is already here'), completed])), () => new Promise<Response>(() => {}))
    await send(fetchMock)

    expect(await screen.findByText('Reading the recorded usage…')).toBeInTheDocument()
    expect(screen.getByText('the answer is already here')).toBeInTheDocument()
    // The internal id is not known yet, so there is no unambiguous request to link to —
    // and the repeatable header id is never used as a substitute.
    expect(screen.queryByRole('link', { name: 'View request' })).not.toBeInTheDocument()
    expect(screen.getByText('req_c2')).toBeInTheDocument()
  })

  it('offers a retry when the record cannot be read, and links by the internal id once it can', async () => {
    let attempts = 0
    const fetchMock = mockApi(() => Promise.resolve(sse([delta('streamed answer'), completed])), () => { attempts += 1; return attempts === 1 ? json({ error: { type: 'internal_error', message: 'boom' } }, 500) : json(record) })
    const { user } = await send(fetchMock)

    expect(await screen.findByText('The recorded usage for this request could not be read.')).toBeInTheDocument()
    // The response survives the failed read, with the retry that reads the same request.
    expect(screen.getByText('streamed answer')).toBeInTheDocument()
    // A read that failed returned no internal id, so no detail link is rendered: the
    // header id would open whichever row the backend resolved for it.
    expect(screen.queryByRole('link', { name: 'View request' })).not.toBeInTheDocument()
    expect(screen.getByText('req_c2')).toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: 'Retry' }))

    expect(await screen.findByText('Input tokens: 11')).toBeInTheDocument()
    expect(await screen.findByText('Cost: $0.000014')).toBeInTheDocument()
    expect(screen.getByRole('link', { name: 'View request' })).toHaveAttribute('href', '/operations/requests/internal-1')
    const calls = recordCalls(fetchMock)
    expect(calls).toHaveLength(2)
    expect(String(calls[1][0])).toBe('/api/admin/v1/projects/p1/operations/requests/req_c2')
  })

  it('does not keep a previous run’s internal link for a new run', async () => {
    // The caller's id repeats here — which is exactly what makes it unusable as a
    // navigation identity — and the second run's record has not been read yet. The panel
    // must not keep showing the first run's internal id, which belongs to another record.
    let reads = 0
    const fetchMock = mockApi(() => Promise.resolve(sse([delta('streamed answer'), completed])), () => { reads += 1; return reads === 1 ? json(record) : new Promise<Response>(() => {}) })
    vi.stubGlobal('fetch', fetchMock)
    const user = userEvent.setup()
    renderPage()
    await fillForm(user)
    await user.click(screen.getByRole('button', { name: 'Send request' }))

    expect(await screen.findByRole('link', { name: 'View request' })).toHaveAttribute('href', '/operations/requests/internal-1')

    await user.type(screen.getByLabelText(/^Input/), 'second request')
    await user.click(screen.getByRole('button', { name: 'Send request' }))

    expect(await screen.findByText('Reading the recorded usage…')).toBeInTheDocument()
    expect(screen.queryByRole('link', { name: 'View request' })).not.toBeInTheDocument()
    // The second run really is a second lookup of the same header id.
    expect(recordCalls(fetchMock)).toHaveLength(2)
  })
})
