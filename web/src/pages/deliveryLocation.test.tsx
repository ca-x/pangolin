import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { ConfirmHost } from '../components'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import { ThemeProvider } from '../theme'
import SystemPage from './SystemPage'

/**
 * `backup_job_targets.object_key` is where a delivery actually landed, and the
 * projection returns it (`src/api/operations_api.rs`), but the run table showed
 * only the target, status, size and error — so "delivered" never said where.
 * A target whose key the projection has not recorded reads `—`.
 */
const json = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }))
const paged = (rows: unknown[]) => json({ data: rows, total: rows.length, offset: 0, limit: 25 })
const project = { id: 'p1', name: 'Project A', slug: 'project-a', owner_user_id: 'u1', is_default: true, enabled: true }
const job = { id: 'j1', kind: 'backup', status: 'succeeded', attempts: 1, fence: 0, due_at: 1_700_000_000, created_at: 1_700_000_000, error_code: null }
const results = [
  { id: 'r1', job_id: 'j1', storage_id: 'target-1', revision: 1, object_key: 'projects/p1/backups/j1.json', status: 'succeeded', error_code: null, byte_size: 2048 },
  { id: 'r2', job_id: 'j1', storage_id: 'target-2', revision: 1, object_key: null, status: 'failed', error_code: 'upstream_failed', byte_size: null },
]

describe('a delivery run names where the archive was written', () => {
  beforeEach(async () => { await i18n.changeLanguage('en') })

  it('shows the object key of each target and — when it was not recorded', async () => {
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      if (path.includes('/backup-target-results')) return paged(results)
      if (path.includes('/jobs')) return paged([job])
      if (path.includes('/operations/storage')) return paged([{ id: 'target-1', name: 'Local backups', kind: 'local', enabled: true }, { id: 'target-2', name: 'Archive', kind: 's3', enabled: true }])
      return paged([])
    }))
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><ThemeProvider><MemoryRouter><ProjectProvider><SystemPage /><ConfirmHost /></ProjectProvider></MemoryRouter></ThemeProvider></QueryClientProvider>)

    await userEvent.click(await screen.findByRole('tab', { name: 'Backups' }))

    // The target line keeps its own fields, and the location sits under it.
    expect(await screen.findByText(/Local backups · Succeeded · 2048 B · —/)).toBeInTheDocument()
    expect(screen.getByText('Archive location: projects/p1/backups/j1.json')).toBeInTheDocument()
    expect(screen.getByText('Archive location: —')).toBeInTheDocument()
  })
})
