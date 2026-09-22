import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import SystemPage from './SystemPage'
import { ThemeProvider } from '../theme'

/**
 * Browser QA found the artifact download answering `permission denied` in the
 * console while the same read worked from the API: the download is a GET
 * (`web/src/pages/SystemPage.tsx:569`) and `api()` attaches `X-Pangolin-CSRF` only to
 * state-changing methods, while `get_artifact` requires it (`actor(..., true)`).
 * The console sends the header rather than the route dropping the requirement —
 * relaxing a CSRF check is not a fix.
 */
vi.mock('sonner', () => ({ toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() } }))

const json = (value: unknown, status = 200) => Promise.resolve(new Response(JSON.stringify(value), { status, headers: { 'Content-Type': 'application/json' } }))
const project = { id: 'p1', name: 'Project', slug: 'project', owner_user_id: 'u1', is_default: true, enabled: true }
const artifact = { id: 'a1', job_id: 'j1', storage_id: 's1', status: 'succeeded', error_code: null, byte_size: 2048, object_key: 'backups/2026-09-21.json', created_at: 1_700_000_000 }

const mockApi = () => vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
  const path = String(input)
  if (path.includes('/permissions')) return json(['*'])
  if (path.includes('settings/orchestration')) return json({ version: 1, affinity_rules: [], session_compaction: {} })
  if (path.includes('/backup/artifacts/')) return json(artifact)
  if (path.includes('/backup/artifacts')) return json({ artifacts: [artifact] })
  if (path.includes('/projects')) return json([project])
  return json({ data: [], total: 0 })
})

const renderPage = (fetchMock: ReturnType<typeof vi.fn>) => {
  vi.stubGlobal('fetch', fetchMock)
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(<QueryClientProvider client={client}><ThemeProvider><MemoryRouter><ProjectProvider><SystemPage /></ProjectProvider></MemoryRouter></ThemeProvider></QueryClientProvider>)
}

describe('downloading an artifact sends the CSRF header the route requires', () => {
  beforeEach(async () => { await i18n.changeLanguage('en'); vi.clearAllMocks() })

  it('carries X-Pangolin-CSRF on the artifact read', async () => {
    const fetchMock = mockApi()
    renderPage(fetchMock)
    await userEvent.click(await screen.findByRole('tab', { name: 'Backups' }))

    await userEvent.click(await screen.findByRole('button', { name: /^Download a1$/ }))

    await waitFor(() => {
      const call = fetchMock.mock.calls.find(([input]) => String(input).includes('/backup/artifacts/a1'))
      expect(call).toBeTruthy()
      const headers = new Headers((call![1] as RequestInit | undefined)?.headers)
      expect(headers.get('X-Pangolin-CSRF')).toBe('1')
    })
  })
})
