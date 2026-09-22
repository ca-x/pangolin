import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import PlaygroundPage from './PlaygroundPage'

/**
 * The playground consumed `response.output_text.delta` and nothing else, then
 * marked EOF as `done`. Every terminal event the Responses protocol can send —
 * `response.failed`, `response.incomplete`, `response.cancelled` and a
 * `response.completed` carrying a non-completed status — therefore displayed as a
 * successful response, including the gateway's own interrupted-stream envelope
 * (`src/orchestration/stream.rs::interrupted_event`). The console must report the
 * outcome the protocol reported, not the HTTP status of the connection.
 *
 * The fixtures are the shapes the gateway actually writes: `stream::encode` emits
 * an `event:` line and the same event name inside `data:`.
 */
const json = (value: unknown, status = 200) => Promise.resolve(new Response(JSON.stringify(value), { status, headers: { 'Content-Type': 'application/json' } }))
const project = { id: 'p1', name: 'Project', slug: 'project', owner_user_id: 'u1', is_default: true, enabled: true }
const models = { data: [{ id: 'qa-fast' }] }
const record = { executions: [], usage: [], cost_items: [] }

/** One SSE frame, framed the way `src/orchestration/stream.rs::encode` frames it. */
const frame = (name: string, payload: unknown) => `event: ${name}\ndata: ${JSON.stringify(payload)}\n\n`
const delta = (text: string) => frame('response.output_text.delta', { type: 'response.output_text.delta', item_id: 'msg_1', output_index: 0, content_index: 0, delta: text })
const created = frame('response.created', { type: 'response.created', response: { id: 'resp_1', status: 'in_progress' } })
const completed = frame('response.completed', { type: 'response.completed', response: { id: 'resp_1', status: 'completed', output: [] } })
const sse = (frames: string[]) => new Response(frames.join(''), { status: 200, headers: { 'Content-Type': 'text/event-stream', 'x-request-id': 'req_c2' } })

/** A stream that stays open until the request is aborted — what Stop has to interrupt. */
const held = (frames: string[], signal: AbortSignal | null | undefined) => new Response(new ReadableStream<Uint8Array>({
  start(controller) {
    const encoder = new TextEncoder()
    for (const value of frames) controller.enqueue(encoder.encode(value))
    signal?.addEventListener('abort', () => controller.error(new DOMException('aborted', 'AbortError')))
  },
}), { status: 200, headers: { 'Content-Type': 'text/event-stream', 'x-request-id': 'req_c2' } })

const mockApi = (answer: (signal: AbortSignal | null | undefined) => Promise<Response>) => vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
  const path = String(input)
  if (path.includes('/api-keys')) return json([{ id: 'key-1', name: 'pk_test_123456', enabled: true, expires_at: null, budget_micros: null, spent_micros: 0 }])
  if (path.includes('/playground/models')) return json(models)
  if (path.includes('/playground/chat')) return answer(init?.signal)
  if (path.includes('/operations/requests/')) return json(record)
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

const gatewayCalls = (fetchMock: ReturnType<typeof vi.fn>) => fetchMock.mock.calls.filter(([input]) => String(input).includes('/playground/chat')).length

/**
 * A stream whose frames are pushed by the test, so a run can be left in flight
 * while the panel moves on. The signal is deliberately not honoured: the race the
 * ownership guard exists for is bytes that were already on their way when another
 * submission took the panel.
 */
const deferredStream = () => {
  let push = (_text: string) => {}
  const body = new ReadableStream<Uint8Array>({ start(controller) { push = (text) => controller.enqueue(new TextEncoder().encode(text)) } })
  return { response: new Response(body, { status: 200, headers: { 'Content-Type': 'text/event-stream', 'x-request-id': 'req_c2' } }), push: (text: string) => push(text) }
}

const send = async (fetchMock: ReturnType<typeof vi.fn>) => {
  const user = userEvent.setup()
  vi.stubGlobal('fetch', fetchMock)
  renderPage()
  await fillForm(user)
  await user.click(screen.getByRole('button', { name: 'Send request' }))
  return user
}

describe('the playground reports the outcome the Responses protocol reported', () => {
  beforeEach(() => { void i18n.changeLanguage('en') })

  it('shows a completed response as completed', async () => {
    const fetchMock = mockApi(() => Promise.resolve(sse([created, delta('all of it'), completed])))
    await send(fetchMock)

    expect(await screen.findByText('all of it')).toBeInTheDocument()
    expect(await screen.findByText('Outcome: Completed')).toBeInTheDocument()
    expect(screen.queryByText('Outcome: Failed')).not.toBeInTheDocument()
    expect(gatewayCalls(fetchMock)).toBe(1)
  })

  it.each([
    ['response.failed', frame('response.failed', { type: 'response.failed', response: { status: 'failed', error: { type: 'upstream_error', message: 'upstream refused the request' } } }), 'Failed', 'upstream refused the request'],
    ['response.incomplete', frame('response.incomplete', { type: 'response.incomplete', response: { id: 'resp_1', status: 'incomplete' } }), 'Incomplete', 'The response is incomplete'],
    ['response.cancelled', frame('response.cancelled', { type: 'response.cancelled', response: { id: 'resp_1', status: 'cancelled' } }), 'Cancelled', 'This response was cancelled'],
    ['response.canceled', frame('response.canceled', { type: 'response.canceled', response: { id: 'resp_1', status: 'canceled' } }), 'Cancelled', 'This response was cancelled'],
    ['response.completed carrying an incomplete status', frame('response.completed', { type: 'response.completed', response: { id: 'resp_1', status: 'incomplete' } }), 'Incomplete', 'The response is incomplete'],
    ['response.completed carrying an error', frame('response.completed', { type: 'response.completed', response: { id: 'resp_1', status: 'completed', error: { type: 'upstream_error', message: 'upstream gave up' } } }), 'Failed', 'upstream gave up'],
    ['the error envelope of the Responses endpoint', frame('error', { type: 'error', error: { type: 'upstream_error', message: 'upstream request failed' } }), 'Failed', 'upstream request failed'],
  ])('shows %s as a failure, never as a completed response', async (_name, terminal, outcome, copy) => {
    const fetchMock = mockApi(() => Promise.resolve(sse([created, terminal])))
    await send(fetchMock)

    expect(await screen.findByText(`Outcome: ${outcome}`)).toBeInTheDocument()
    // The public message of the envelope when it has one, and the status-specific
    // localized copy when it does not.
    expect(await screen.findByText(new RegExp(copy))).toBeInTheDocument()
    expect(screen.queryByText('Outcome: Completed')).not.toBeInTheDocument()
    expect(gatewayCalls(fetchMock)).toBe(1)
  })

  it('reports the gateway\'s interrupted stream envelope as a failure', async () => {
    // `stream::interrupted_event` for `/v1/responses`: the transport died after the
    // first downstream event, so the gateway hands the client this envelope.
    const interrupted = frame('response.failed', { type: 'response.failed', response: { status: 'failed', error: { type: 'upstream_error', message: 'upstream stream interrupted' } } })
    const fetchMock = mockApi(() => Promise.resolve(sse([created, delta('half an answer'), interrupted])))
    await send(fetchMock)

    expect(await screen.findByText('Outcome: Failed')).toBeInTheDocument()
    // The public half of the envelope: its type in the title, its message below.
    expect(await screen.findByText('Failed · upstream_error')).toBeInTheDocument()
    expect(await screen.findByText('upstream stream interrupted')).toBeInTheDocument()
    expect(screen.queryByText('Outcome: Completed')).not.toBeInTheDocument()
  })

  it('keeps the text that arrived before a failure next to the failure', async () => {
    const fetchMock = mockApi(() => Promise.resolve(sse([created, delta('partial text that arrived first'), frame('response.failed', { type: 'response.failed', response: { status: 'failed', error: { type: 'upstream_error', message: 'upstream refused the request' } } })])))
    await send(fetchMock)

    expect(await screen.findByText('partial text that arrived first')).toBeInTheDocument()
    expect(await screen.findByText('upstream refused the request')).toBeInTheDocument()
    expect(screen.queryByText('The response carried no text output.')).not.toBeInTheDocument()
  })

  it('does not claim a stream that ended without a terminal event completed', async () => {
    const fetchMock = mockApi(() => Promise.resolve(sse([created, delta('truncated answer')])))
    await send(fetchMock)

    expect(await screen.findByText('Outcome: Interrupted')).toBeInTheDocument()
    expect(await screen.findByText('The stream ended without a terminal event, so this response is not known to be complete.')).toBeInTheDocument()
    expect(screen.queryByText('Outcome: Completed')).not.toBeInTheDocument()
    // The server committed bytes and never retries them, so the console must not
    // ask again either.
    expect(gatewayCalls(fetchMock)).toBe(1)
  })

  it('reports an operator stop as stopped and keeps what had already streamed', async () => {
    const fetchMock = mockApi((signal) => Promise.resolve(held([created, delta('text before the stop')], signal)))
    const user = await send(fetchMock)

    expect(await screen.findByText('text before the stop')).toBeInTheDocument()
    await user.click(await screen.findByRole('button', { name: 'Stop' }))

    expect(await screen.findByText('Outcome: Stopped')).toBeInTheDocument()
    expect(await screen.findByText('You stopped this request. What is shown above is what had arrived before you stopped it.')).toBeInTheDocument()
    expect(screen.getByText('text before the stop')).toBeInTheDocument()
    expect(screen.queryByText('Outcome: Completed')).not.toBeInTheDocument()
    expect(gatewayCalls(fetchMock)).toBe(1)
  })

  it('keeps the first terminal when the stream reports another one and more text after it', async () => {
    // The gateway returns as soon as it writes a terminal, but a stream that keeps
    // talking must not be able to rewrite the outcome — and text that arrives after
    // the terminal is not part of the response the protocol described.
    const fetchMock = mockApi(() => Promise.resolve(sse([
      created,
      delta('before the failure'),
      frame('response.failed', { type: 'response.failed', response: { status: 'failed', error: { type: 'upstream_error', message: 'upstream refused the request' } } }),
      delta('after the failure'),
      completed,
    ])))
    await send(fetchMock)

    expect(await screen.findByText('Outcome: Failed')).toBeInTheDocument()
    expect(await screen.findByText('upstream refused the request')).toBeInTheDocument()
    expect(screen.getByText('before the failure')).toBeInTheDocument()
    expect(screen.queryByText(/after the failure/)).not.toBeInTheDocument()
    expect(screen.queryByText('Outcome: Completed')).not.toBeInTheDocument()
  })

  it('lets no run mutate the panel after another submission took it', async () => {
    const first = deferredStream()
    const answers = [first.response, sse([delta('answer from the second request'), completed])]
    const fetchMock = mockApi(() => Promise.resolve(answers.shift() as Response))
    const user = await send(fetchMock)

    // The first run reaches a terminal and leaves its body open: the panel is free
    // again, and the operator sends the next request.
    first.push(frame('response.failed', { type: 'response.failed', response: { status: 'failed', error: { type: 'upstream_error', message: 'upstream refused the request' } } }))
    expect(await screen.findByText('Outcome: Failed')).toBeInTheDocument()

    await user.type(screen.getByLabelText(/^Input/), 'second request')
    await user.click(screen.getByRole('button', { name: 'Send request' }))

    expect(await screen.findByText('Outcome: Completed')).toBeInTheDocument()
    expect(await screen.findByText('answer from the second request')).toBeInTheDocument()
    expect(gatewayCalls(fetchMock)).toBe(2)

    // The superseded run delivers its buffered bytes now. They belong to the run
    // that asked for them, not to this panel. Its own terminal already decided its
    // outcome, so the frames are ignored; the run-id checks in the reader are the
    // second layer, for bytes that arrive before that run's terminal.
    first.push(delta('stale text from the first request') + completed)

    // Give the superseded run every chance to write: it must not.
    await new Promise((resolve) => setTimeout(resolve, 50))
    expect(screen.queryByText(/stale text from the first request/)).not.toBeInTheDocument()
    expect(screen.getByText('answer from the second request')).toBeInTheDocument()
    expect(screen.getByText('Outcome: Completed')).toBeInTheDocument()
    expect(screen.queryByText('Outcome: Failed')).not.toBeInTheDocument()
  })
})
