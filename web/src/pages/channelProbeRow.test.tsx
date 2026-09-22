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
    if (path.includes('operations/probes')) return json({ data: probed ? [{ id: 'pr1', provider_id: 'c1', model: 'gpt-4o-mini', success: 1, status_code: 200, latency_ms: 12, probed_at: 1_700_000_000, error: null }] : [], total: probed ? 1 : 0 })
    if (path.includes('operations/health') || path.includes('operations/credential-health')) return json({ data: [], total: 0 })
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
      expect(JSON.parse(String((call![1] as RequestInit).body))).toEqual({ provider_id: 'c1' })
    })

    // The outcome lands where the decision was made, with the model the probe
    // actually used — the console does not choose it, the backend does.
    expect(await within(dialog).findByText('Succeeded')).toBeInTheDocument()
    expect(within(dialog).getByText('Latency: 12 ms')).toBeInTheDocument()
    expect(within(dialog).getByText('Model: gpt-4o-mini')).toBeInTheDocument()
    expect(within(dialog).getByText('Status code: 200')).toBeInTheDocument()
  })
})
