import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter, Route, Routes } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import { RequestDetailPage } from './OperationsPage'
import { parseConversation, PayloadViewer } from './PayloadViewer'

const envelope = (event: string, data: unknown, terminal = false) => JSON.stringify({ version: 1, event, data, terminal })
const json = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }))
const project = { id: 'p1', name: 'Project', slug: 'project', owner_user_id: 'u1', is_default: true, enabled: true }

function renderDetail(detail: Record<string, unknown>) {
  vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
    const path = String(input)
    if (path.endsWith('/projects')) return json([project])
    if (path.includes('/permissions')) return json(['*'])
    if (path.includes('/observability/requests/')) return json(detail)
    if (path.includes('/operations/requests/')) return json({ executions: [], usage: [], cost_items: [] })
    if (path.includes('/operations/traces')) return json({ data: [], total: 0 })
    return json({ data: [], total: 0 })
  }))
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(<QueryClientProvider client={client}><MemoryRouter initialEntries={['/operations/requests/internal-1']}><ProjectProvider><Routes><Route path="/operations/requests/:id" element={<RequestDetailPage />} /></Routes></ProjectProvider></MemoryRouter></QueryClientProvider>)
}

describe('the stored payload viewer', () => {
  beforeEach(async () => { await i18n.changeLanguage('en') })

  it('renders versioned stream envelopes in order and marks the explicit terminal', async () => {
    const responseRaw = [
      envelope('response.output_text.delta', { type: 'response.output_text.delta', delta: 'hello' }),
      envelope('response.completed', { type: 'response.completed', response: { status: 'completed' } }, true),
    ].join('\n')

    render(<PayloadViewer requestId="req-1" endpoint="/v1/responses" stream captured requestRaw='{"model":"demo","input":"hello","stream":true}' responseRaw={responseRaw} />)
    const response = await screen.findByRole('region', { name: 'Response chunks' })

    const chunks = within(response).getAllByRole('listitem')
    expect(chunks).toHaveLength(2)
    expect(within(chunks[0]).getByText('response.output_text.delta')).toBeInTheDocument()
    expect(within(chunks[0]).queryByText('Terminal')).not.toBeInTheDocument()
    expect(within(chunks[1]).getByText('response.completed')).toBeInTheDocument()
    expect(within(chunks[1]).getByText('Terminal')).toBeInTheDocument()
  })

  it('renders legacy newline JSON without guessing that the last chunk is terminal', async () => {
    const responseRaw = [
      JSON.stringify({ choices: [{ delta: { content: 'first' } }] }),
      JSON.stringify({ choices: [{ delta: { content: 'last captured' } }] }),
    ].join('\n')

    render(<PayloadViewer requestId="req-legacy" endpoint="/v1/chat/completions" stream captured requestRaw='{"messages":[]}' responseRaw={responseRaw} />)
    const response = await screen.findByRole('region', { name: 'Response chunks' })

    const chunks = within(response).getAllByRole('listitem')
    expect(chunks).toHaveLength(2)
    expect(chunks[0]).toHaveTextContent('first')
    expect(chunks[1]).toHaveTextContent('last captured')
    expect(screen.queryByText('Terminal')).not.toBeInTheDocument()
  })

  it('parses standard SSE frames and marks the protocol terminal sentinel', async () => {
    const responseRaw = [
      'event: response.output_text.delta\ndata: {"type":"response.output_text.delta","delta":"hello"}',
      'data: [DONE]',
      '',
    ].join('\n\n')

    render(<PayloadViewer requestId="req-sse" endpoint="/v1/chat/completions" stream captured requestRaw='{"messages":[]}' responseRaw={responseRaw} />)
    const response = await screen.findByRole('region', { name: 'Response chunks' })

    const chunks = within(response).getAllByRole('listitem')
    expect(chunks).toHaveLength(2)
    expect(within(chunks[0]).getByText('response.output_text.delta')).toBeInTheDocument()
    expect(within(chunks[1]).getByText('[DONE]')).toBeInTheDocument()
    expect(within(chunks[1]).getByText('Terminal')).toBeInTheDocument()
  })

  it('defaults a message-shaped body to a safe conversation and keeps its JSON view', async () => {
    const requestRaw = JSON.stringify({
      model: 'demo',
      messages: [
        { role: 'system', content: 'Be exact.' },
        { role: 'user', content: '<img src="https://tracker.invalid/pixel"> Hello' },
      ],
    })

    render(<PayloadViewer requestId="req-chat" endpoint="/v1/chat/completions" stream={false} captured requestRaw={requestRaw} responseRaw='{"id":"answer"}' />)
    const request = await screen.findByRole('region', { name: 'Request payload' })

    expect(within(request).getByRole('region', { name: 'Request conversation' })).toHaveTextContent('Be exact.')
    expect(within(request).getByText('<img src="https://tracker.invalid/pixel"> Hello')).toBeInTheDocument()
    expect(within(request).queryByRole('img')).not.toBeInTheDocument()
    await userEvent.click(within(request).getByRole('tab', { name: 'JSON' }))
    expect(within(request).getByText(/"messages"/)).toBeInTheDocument()
  })

  it('downloads the exact stored body instead of a reconstructed document', async () => {
    const requestRaw = '{"model":"demo", "messages":[]}'
    let downloaded: Blob | null = null
    vi.stubGlobal('URL', { ...URL, createObjectURL: vi.fn((blob: Blob) => { downloaded = blob; return 'blob:payload' }), revokeObjectURL: vi.fn() })
    const click = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => {})

    render(<PayloadViewer requestId="req-download" endpoint="/v1/chat/completions" stream={false} captured requestRaw={requestRaw} responseRaw='{"ok":true}' />)
    await userEvent.click(await screen.findByRole('button', { name: 'Download request payload' }))

    expect(click).toHaveBeenCalledTimes(1)
    expect(downloaded).not.toBeNull()
    expect(await downloaded!.text()).toBe(requestRaw)
  })

  it('builds a same-origin curl preview with a placeholder and strips legacy secrets', async () => {
    const requestRaw = JSON.stringify({
      model: 'demo',
      messages: [{ role: 'user', content: "it's safe" }],
      authorization: 'Bearer historical-secret',
      apikey: 'historical-api-key',
      b64_json: 'historical-media',
    })

    render(<PayloadViewer requestId="req-curl" endpoint="/v1/chat/completions" stream={false} captured requestRaw={requestRaw} responseRaw='{"ok":true}' />)
    await userEvent.click(await screen.findByRole('button', { name: 'Preview cURL' }))
    const dialog = await screen.findByRole('dialog', { name: 'cURL preview' })

    expect(dialog).toHaveTextContent('/v1/chat/completions')
    expect(dialog).toHaveTextContent('$PANGOLIN_API_KEY')
    expect(dialog).toHaveTextContent("it'\"'\"'s safe")
    expect(dialog).toHaveTextContent('Captured bodies are sanitized and may not be replayable.')
    expect(dialog).not.toHaveTextContent('historical-secret')
    expect(dialog).not.toHaveTextContent('historical-api-key')
    expect(dialog).not.toHaveTextContent('historical-media')
    expect(within(dialog).getByRole('button', { name: 'Copy cURL' })).toBeInTheDocument()
  })

  it('paginates a long chunk stream without mounting every event at once', async () => {
    const responseRaw = Array.from({ length: 25 }, (_, index) => envelope('response.output_text.delta', { delta: `part-${index + 1}` })).join('\n')
    render(<PayloadViewer requestId="req-pages" endpoint="/v1/responses" stream captured requestRaw='{"input":"hello"}' responseRaw={responseRaw} />)
    const response = await screen.findByRole('region', { name: 'Response chunks' })

    expect(within(response).getAllByRole('listitem')).toHaveLength(20)
    expect(within(response).getAllByRole('listitem')[0]).toHaveTextContent('part-1')
    expect(response).not.toHaveTextContent('part-21')
    await userEvent.click(within(response).getByRole('button', { name: 'Next chunks page' }))
    expect(within(response).getAllByRole('listitem')).toHaveLength(5)
    expect(within(response).getAllByRole('listitem')[0]).toHaveTextContent('part-21')
  })

  it('uses the payload viewer from the request detail instead of parsing a stream as one JSON document', async () => {
    renderDetail({
      id: 'internal-1', request_id: 'external-1', trace_id: 'trace-1', started_at: 1, finished_at: 2,
      endpoint: '/v1/responses', provider: 'openai', requested_model: 'demo', resolved_model: 'demo',
      status_code: 200, error_kind: null, latency_ms: 10, ttft_ms: 1, input_tokens: 1, output_tokens: 1,
      cached_tokens: 0, cache_write_tokens: 0, reasoning_tokens: 0, stream: true, cost_micros: 1, api_key_id: 'key-1',
      payload_captured: true, request_json: '{"model":"demo","input":"hello","stream":true}',
      response_json: [envelope('response.output_text.delta', { type: 'response.output_text.delta', delta: 'hi' }), envelope('response.completed', { type: 'response.completed', response: { status: 'completed' } }, true)].join('\n'),
    })

    expect(await screen.findByRole('region', { name: 'Response chunks' })).toBeInTheDocument()
    expect(screen.getByText('Terminal')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Preview cURL' })).toBeInTheDocument()
  })

  it('keeps the secure policy alert when payload capture was off', async () => {
    renderDetail({
      id: 'internal-off', request_id: 'external-off', trace_id: 'trace-off', started_at: 1, finished_at: 2,
      endpoint: '/v1/chat/completions', provider: null, requested_model: 'demo', resolved_model: null,
      status_code: null, error_kind: null, latency_ms: 10, ttft_ms: null, input_tokens: 0, output_tokens: 0,
      cached_tokens: 0, cache_write_tokens: 0, reasoning_tokens: 0, stream: null, cost_micros: 0, api_key_id: null,
      payload_captured: false, request_json: null, response_json: null,
    })

    expect(await screen.findByRole('alert')).toHaveTextContent('This is the secure default.')
    expect(screen.queryByRole('region', { name: 'Request payload' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Preview cURL' })).not.toBeInTheDocument()
  })

  it('falls back to raw text for a malformed captured stream without inventing a terminal', async () => {
    render(<PayloadViewer requestId="req-malformed" endpoint="/v1/chat/completions" stream captured requestRaw='{"messages":[]}' responseRaw={'{"choices":[]}\nnot-json'} />)
    const response = await screen.findByRole('region', { name: 'Response payload' })

    expect(within(response).getByRole('region', { name: 'Raw JSON' })).toHaveTextContent('not-json')
    expect(within(response).queryByText('Terminal')).not.toBeInTheDocument()
  })

  it('normalizes the minimum Responses, Anthropic and Gemini conversation shapes', () => {
    expect(parseConversation('{"input":"hello"}', '/v1/responses')?.messages).toEqual([
      expect.objectContaining({ role: 'user', content: 'hello' }),
    ])
    expect(parseConversation(JSON.stringify({ system: 'Be exact.', messages: [{ role: 'user', content: [{ type: 'text', text: 'question' }] }] }), '/v1/messages')?.messages).toEqual([
      expect.objectContaining({ role: 'system', content: 'Be exact.' }),
      expect.objectContaining({ role: 'user', content: 'question' }),
    ])
    expect(parseConversation(JSON.stringify({ contents: [{ role: 'user', parts: [{ text: 'Gemini question' }] }] }), '/v1beta/models:generateContent')?.messages).toEqual([
      expect.objectContaining({ role: 'user', content: 'Gemini question' }),
    ])
  })
})
