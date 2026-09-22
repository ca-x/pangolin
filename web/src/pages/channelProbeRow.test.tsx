import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import ChannelsPage from './ChannelsPage'
import { ConfirmHost } from '../components'

/**
 * The reference product probes a channel from its own row and shows the outcome
 * where the operator asked for it (`features/channels/components/channels-test-dialog.tsx`
 * keeps a per-model status with latency and error). Pangolin's probe existed but was
 * detached: a picker at the top of the Probes tab enqueued a durable job and the
 * operator had to go and read a different tab to find out what happened. This pins
 * the per-row action and the inline outcome.
 */
vi.mock('sonner', () => ({ toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() } }))

const json = (value: unknown, status = 200) => Promise.resolve(new Response(JSON.stringify(value), { status, headers: { 'Content-Type': 'application/json' } }))
const project = { id: 'p1', name: 'Project', slug: 'project', owner_user_id: 'u1', is_default: true, enabled: true }
const channel = { id: 'c1', name: 'openai-prod', kind: 'openai', base_url: 'https://api.openai.com/v1', enabled: true }
const model = { id: 'm1', provider_id: 'c1', public_name: 'fast', upstream_name: 'gpt-4o-mini', enabled: true, lifecycle: 'active' }

const renderPage = (fetchMock: ReturnType<typeof vi.fn>) => {
  vi.stubGlobal('fetch', fetchMock)
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <ProjectProvider><ChannelsPage /><ConfirmHost /></ProjectProvider>
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

/** A probe the job has not run yet, then the row it records once it has. */
const mockApi = () => {
  let probed = false
  return vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
    const path = String(input)
    if (path.endsWith('/projects')) return json([project])
    if (path.includes('/permissions')) return json(['*'])
    if (path.includes('operations/probe') && init?.method === 'POST') { probed = true; return json({ job_id: 'j1' }) }
    if (path.includes('operations/probes')) return json({ data: probed ? [{ id: 'pr1', provider_id: 'c1', model: 'gpt-4o-mini', success: 1, status_code: 200, latency_ms: 110, ttft_ms: 10, output_tokens: 10, probed_at: 1_700_000_000, error: null }] : [], total: probed ? 1 : 0 })
    if (path.includes('operations/health') || path.includes('operations/credential-health')) return json({ data: [], total: 0 })
    if (path.includes('operations/models')) return json({ data: [model], total: 1 })
    return json({ data: [channel], total: 1 })
  })
}

describe('a channel can be probed from its own row', () => {
  beforeEach(async () => { await i18n.changeLanguage('en'); vi.clearAllMocks() })

  it('enqueues the probe for that channel and reports the outcome inline', async () => {
    const fetchMock = mockApi()
    renderPage(fetchMock)

    // The page renders the row in the desktop table and again in the mobile card
    // list; act on the table, which is where an operator on a wide screen is.
    const table = within(await screen.findByRole('region', { name: 'Channels' }))
    await userEvent.click(await table.findByRole('button', { name: 'Test openai-prod' }))
    const dialog = await screen.findByRole('dialog')

    await userEvent.click(within(dialog).getByRole('button', { name: 'Run probe' }))

    await waitFor(() => {
      const call = fetchMock.mock.calls.find(([input, init]) => String(input).includes('operations/probe') && (init as RequestInit | undefined)?.method === 'POST')
      expect(call).toBeTruthy()
      expect(JSON.parse(String((call![1] as RequestInit).body))).toEqual({ provider_id: 'c1', model_id: 'm1' })
    })

    // The outcome lands where the decision was made, with the model selected by
    // the operator and validated by the backend.
    expect((await within(dialog).findAllByText('Succeeded')).length).toBeGreaterThan(0)
    expect(within(dialog).getByText('Latency: 110 ms')).toBeInTheDocument()
    expect(within(dialog).getByText('Model: gpt-4o-mini')).toBeInTheDocument()
    expect(within(dialog).getByText('Status code: 200')).toBeInTheDocument()
    expect(within(dialog).getByText('Success rate')).toBeInTheDocument()
    expect(within(dialog).getByText('100%')).toBeInTheDocument()
    expect(within(dialog).getByText('Average TTFT')).toBeInTheDocument()
    expect(within(dialog).getByText('10 ms')).toBeInTheDocument()
    expect(within(dialog).getByText('Tokens/s')).toBeInTheDocument()
    expect(within(dialog).getByText('100.0')).toBeInTheDocument()
  })

  it('runs all eligible channels with bounded concurrency and reports each channel', async () => {
    const channels = Array.from({ length: 5 }, (_, index) => ({ ...channel, id: `c${index + 1}`, name: `Channel ${index + 1}` }))
    const models = channels.map((item, index) => ({ ...model, id: `m${index + 1}`, provider_id: item.id, public_name: `model-${index + 1}` }))
    let active = 0
    let maximum = 0
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      if (path.includes('operations/probe') && init?.method === 'POST') {
        active += 1
        maximum = Math.max(maximum, active)
        return new Promise<Response>((resolve) => setTimeout(() => { active -= 1; resolve(new Response(JSON.stringify({ id: 'job' }), { status: 200, headers: { 'Content-Type': 'application/json' } })) }, 20))
      }
      if (path.includes('operations/probes') || path.includes('operations/health') || path.includes('operations/credential-health')) return json({ data: [], total: 0 })
      if (path.includes('operations/models')) return json({ data: models, total: models.length })
      if (path.includes('operations/channels')) return json({ data: channels, total: channels.length })
      return json({ data: [], total: 0 })
    })
    renderPage(fetchMock)

    await userEvent.click(await screen.findByRole('button', { name: 'Test all channels' }))
    await waitFor(() => expect(fetchMock.mock.calls.filter(([input, init]) => String(input).includes('operations/probe') && (init as RequestInit | undefined)?.method === 'POST')).toHaveLength(5))
    expect(maximum).toBeLessThanOrEqual(3)
    for (const item of channels) expect(await screen.findByText(`${item.name}: Queued`)).toBeInTheDocument()
  })
})
