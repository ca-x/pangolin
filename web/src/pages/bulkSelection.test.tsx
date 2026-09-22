import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import { ResourcePage } from './shared'

/**
 * The reference product selects rows in a channel, model or key table, shows how
 * many are selected, and offers the bulk action there — its own table carries a
 * checkbox column and a select-all header
 * (`frontend/src/features/channels/components/channels-table.tsx`). Pangolin's
 * only bulk control asked the operator to paste comma-separated ids into a form
 * beside the table, which is why the report's row 1 calls it a gap. This pins the
 * row selection and the bar that drives the same `bulk-toggle` endpoint.
 */
vi.mock('sonner', () => ({ toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() } }))

const json = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }))
const project = { id: 'p1', name: 'Project', slug: 'project', owner_user_id: 'u1', is_default: true, enabled: true }
const rows = [
  { id: 'c1', name: 'openai-prod', kind: 'openai', enabled: true },
  { id: 'c2', name: 'anthropic-prod', kind: 'anthropic', enabled: true },
]

const renderPage = (fetchMock: ReturnType<typeof vi.fn>) => {
  vi.stubGlobal('fetch', fetchMock)
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <ProjectProvider>
          <ResourcePage resource="channels" title="Channels" description="" empty="" selectable="channels" columns={[{ key: 'name', label: 'Name' }]} fields={[{ key: 'name', label: 'Name' }]} />
        </ProjectProvider>
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

const mockApi = () => vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
  const path = String(input)
  if (path.endsWith('/projects')) return json([project])
  if (path.includes('/permissions')) return json(['*'])
  if (path.includes('bulk-toggle')) return init?.method === 'POST' ? json({ changed: 2 }) : json({ data: [], total: 0 })
  return json({ data: rows, total: rows.length })
})

describe('rows can be selected and acted on together', () => {
  beforeEach(async () => { await i18n.changeLanguage('en'); vi.clearAllMocks() })

  it('selects a row and sends the selection to the bulk endpoint', async () => {
    const fetchMock = mockApi()
    renderPage(fetchMock)

    await userEvent.click(await screen.findByRole('checkbox', { name: 'Select openai-prod' }))
    await userEvent.click(screen.getByRole('checkbox', { name: 'Select anthropic-prod' }))
    expect(screen.getByText('2 selected')).toBeInTheDocument()

    await userEvent.click(screen.getByRole('button', { name: 'Disable' }))

    await waitFor(() => {
      const call = fetchMock.mock.calls.find(([input, init]) => String(input).includes('bulk-toggle') && (init as RequestInit | undefined)?.method === 'POST')
      expect(call).toBeTruthy()
      expect(JSON.parse(String((call![1] as RequestInit).body))).toEqual({ resource: 'channels', ids: ['c1', 'c2'], enabled: false })
    })
  })

  it('selects every row on the page from the header, and clears again', async () => {
    renderPage(mockApi())

    await userEvent.click(await screen.findByRole('checkbox', { name: 'Select all' }))
    expect(screen.getByText('2 selected')).toBeInTheDocument()

    await userEvent.click(screen.getByRole('checkbox', { name: 'Select all' }))
    expect(screen.queryByText('2 selected')).not.toBeInTheDocument()
  })

  it('does not offer selection on a table that did not ask for it', async () => {
    const fetchMock = mockApi()
    vi.stubGlobal('fetch', fetchMock)
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(
      <QueryClientProvider client={client}>
        <MemoryRouter>
          <ProjectProvider>
            <ResourcePage resource="channels" title="Channels" description="" empty="" columns={[{ key: 'name', label: 'Name' }]} fields={[{ key: 'name', label: 'Name' }]} />
          </ProjectProvider>
        </MemoryRouter>
      </QueryClientProvider>,
    )
    // The page renders the row in the desktop table and again in the mobile card
    // list, so the text appears twice; what matters is that neither carries a
    // selection control.
    expect((await screen.findAllByText('openai-prod')).length).toBeGreaterThan(0)
    expect(screen.queryByRole('checkbox')).not.toBeInTheDocument()
  })
})
