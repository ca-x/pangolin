import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import ChannelsPage from './ChannelsPage'
import PromptsPage from './PromptsPage'

/**
 * `bulk-toggle` accepts `resource: "prompts"` and `resource: "protection"`
 * (`src/api/operations_api.rs`), and the console offered the action for channels
 * and credentials only. The panel is the same one, so the three resources are
 * checked through the same control: the count decides whether it appears, the
 * body names the resource, and the ids are the ones the operator typed.
 */
const json = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }))
const paged = (rows: unknown[]) => json({ data: rows, total: rows.length, offset: 0, limit: 25 })
const project = { id: 'p1', name: 'Project A', slug: 'project-a', owner_user_id: 'u1', is_default: true, enabled: true }
const renderPage = (page: React.ReactElement) => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider>{page}</ProjectProvider></MemoryRouter></QueryClientProvider>)
}
const posts = (fetchMock: ReturnType<typeof vi.fn>) => fetchMock.mock.calls.filter(([, init]) => (init as RequestInit | undefined)?.method === 'POST')

const bulk = async (ids: string) => {
  await userEvent.type(await screen.findByLabelText(/Resource IDs/), ids)
  await userEvent.click(screen.getByRole('combobox', { name: 'Action' }))
  await userEvent.click(await screen.findByRole('option', { name: 'Disable' }))
  await userEvent.click(screen.getByRole('button', { name: 'Apply' }))
}

describe('bulk enable or disable covers prompts and protection rules', () => {
  beforeEach(async () => { await i18n.changeLanguage('en') })

  it('sends the prompt ids the operator typed', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['project:manage'])
      if (path.includes('/operations/prompts')) return paged([{ id: 'pr1', name: 'Greeting', role: 'system', content: 'hi', activation: { version: 1 }, enabled: true }])
      return paged([])
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage(<PromptsPage />)

    await bulk('pr1, pr2')

    await waitFor(() => expect(posts(fetchMock)).toHaveLength(1))
    const [path, init] = posts(fetchMock)[0] as [string, RequestInit]
    expect(path).toBe('/api/admin/v1/projects/p1/operations/bulk-toggle')
    expect(JSON.parse(String(init.body))).toEqual({ resource: 'prompts', ids: ['pr1', 'pr2'], enabled: false })
  })

  it('sends the protection-rule ids from the protection tab', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['project:manage'])
      if (path.includes('/operations/protection')) return paged([{ id: 'rule1', name: 'Card numbers', content_pattern: '\\d{16}', action: 'redact', enabled: true }])
      return paged([])
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage(<PromptsPage />)

    await userEvent.click(await screen.findByRole('tab', { name: 'Protection' }))
    await bulk('rule1')

    await waitFor(() => expect(posts(fetchMock)).toHaveLength(1))
    const [, init] = posts(fetchMock)[0] as [string, RequestInit]
    expect(JSON.parse(String(init.body))).toEqual({ resource: 'protection', ids: ['rule1'], enabled: false })
  })

  it('still sends the channel resource it always did', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['project:manage'])
      if (path.includes('/operations/channels')) return paged([{ id: 'c1', name: 'openai-prod', kind: 'openai', enabled: true }])
      return paged([])
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage(<ChannelsPage />)

    await bulk('c1')

    await waitFor(() => expect(posts(fetchMock)).toHaveLength(1))
    const [, init] = posts(fetchMock)[0] as [string, RequestInit]
    expect(JSON.parse(String(init.body))).toEqual({ resource: 'channels', ids: ['c1'], enabled: false })
  })
})
