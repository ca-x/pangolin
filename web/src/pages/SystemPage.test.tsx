import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { toast } from 'sonner'
import i18n from '../i18n'
import { ConfirmHost } from '../components'
import { ProjectProvider } from '../project'
import { ThemeProvider } from '../theme'
import SystemPage, { LoggingPolicy } from './SystemPage'

/**
 * The backup tab is a workflow, not three text boxes: an export downloads a
 * file, a delivery run is a queued job that leaves the restore input alone, and
 * the jobs tab is a live list with formatted times and actions that enqueue new
 * work instead of writing a status. Every assertion here is about what the
 * operator sees or what the console sends.
 */
vi.mock('sonner', () => ({ toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() } }))

const json = (value: unknown, status = 200) => Promise.resolve(new Response(JSON.stringify(value), { status, headers: { 'Content-Type': 'application/json' } }))
const paged = (rows: unknown[], total = rows.length) => json({ data: rows, total, offset: 0, limit: 25 })
const project = { id: 'p1', name: 'Project A', slug: 'project-a', owner_user_id: 'u1', is_default: true, enabled: true }
const artifact = { version: 1, id: 'artifact-1234abcd', project_id: 'p1', created_at: 1_700_000_000, digest: 'digest', resources: ['providers', 'api_keys'], envelope: 'envelope' }
const job = (overrides: Record<string, unknown>) => ({ id: 'j1', kind: 'backup', status: 'pending', attempts: 0, fence: 0, due_at: 1_700_000_000, created_at: 1_700_000_000, error_code: null, ...overrides })

const renderPage = (permissions: string[] = ['*']) => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <ThemeProvider>
        <MemoryRouter>
          <ProjectProvider><SystemPage /><ConfirmHost /></ProjectProvider>
        </MemoryRouter>
      </ThemeProvider>
    </QueryClientProvider>,
  )
  return client
}

/** Records every anchor click, so a download is observable instead of assumed. */
function spyDownloads() {
  const clicked: Array<{ download: string; href: string }> = []
  const click = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function (this: HTMLAnchorElement) {
    clicked.push({ download: this.download, href: this.href })
  })
  return { clicked, click }
}

describe('backup and restore are a workflow', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en')
    vi.clearAllMocks()
  })

  it('lists the project artifact history and re-downloads the payload on demand', async () => {
    const { clicked } = spyDownloads()
    const listed = {
      id: 'artifact-history',
      digest: 'digest',
      created_at: 1_700_000_000,
      manifest: { version: 1, resources: ['providers'] },
    }
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      if (path.endsWith('/backup/artifacts')) return json({ artifacts: [listed] })
      if (path.endsWith('/backup/artifacts/artifact-history')) return json(artifact)
      if (init?.method === 'POST') return json({ id: 'job-1' })
      return paged([])
    }))
    renderPage()
    await userEvent.click(await screen.findByRole('tab', { name: 'Backups' }))

    // The history comes from the route, not from what this session happened to
    // export: before this the panel showed only its own exports and said so.
    await waitFor(() => expect(document.body.textContent).toContain('Artifact history'))
    expect(await screen.findByText('Artifact history')).toBeInTheDocument()
    expect(await screen.findByText('artifact-history')).toBeInTheDocument()

    await userEvent.click(await screen.findByRole('button', { name: /artifact-history/ }))
    await waitFor(() => expect(clicked).toHaveLength(1))
    expect(clicked[0].download).toBe('pangolin-backup-project-a-artifact.json')
  })
  it('downloads the exported artifact and never reports it as saved', async () => {
    const { clicked } = spyDownloads()
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      if (path.endsWith('/backup/export')) return json(artifact)
      if (init?.method === 'POST') return json({ id: 'job-1' })
      return paged([])
    }))
    renderPage()
    await userEvent.click(await screen.findByRole('tab', { name: 'Backups' }))
    await userEvent.click(await screen.findByRole('button', { name: 'Export' }))

    await waitFor(() => expect(clicked).toHaveLength(1))
    expect(clicked[0].download).toBe('pangolin-backup-project-a-artifact.json')
    expect(toast.success).toHaveBeenCalledWith('Backup artifact exported', expect.objectContaining({ description: expect.stringContaining('2 tables') }))
    expect(vi.mocked(toast.success).mock.calls.map(([message]) => message)).not.toContain('Saved')
  })

  it('sends the tables chosen in the picker and the targets that exist', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      if (path.endsWith('/backup/export')) return json(artifact)
      if (path.endsWith('/backup/run')) return json({ id: 'job-3' })
      if (path.includes('/operations/storage')) return paged([{ id: 'target-1', name: 'Local backups', kind: 'local', enabled: true }, { id: 'target-2', name: 'Archive', kind: 's3', enabled: false }])
      if (init?.method === 'POST') return json({})
      return paged([])
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    await userEvent.click(await screen.findByRole('tab', { name: 'Backups' }))

    // The picker starts from the configuration set, and history is opt-in.
    await userEvent.click(await screen.findByRole('checkbox', { name: 'requests' }))
    await userEvent.click(screen.getByRole('button', { name: 'Export' }))
    await waitFor(() => expect(fetchMock.mock.calls.filter(([path]) => String(path).includes('/backup/export'))).toHaveLength(1))
    const [, exportInit] = fetchMock.mock.calls.find(([path]) => String(path).includes('/backup/export')) as [string, RequestInit]
    const exported = JSON.parse(String(exportInit.body)).resources as string[]
    expect(exported).toContain('providers')
    expect(exported).toContain('requests')
    expect(exported).not.toContain('usage_logs')

    // Only an enabled storage target can receive a delivery, and the id comes from the row.
    await userEvent.click(await screen.findByRole('checkbox', { name: /Local backups/ }))
    await userEvent.click(screen.getByRole('button', { name: 'Run now' }))
    await waitFor(() => expect(fetchMock.mock.calls.filter(([path]) => String(path).includes('/backup/run'))).toHaveLength(1))
    const [, runInit] = fetchMock.mock.calls.find(([path]) => String(path).includes('/backup/run')) as [string, RequestInit]
    expect(JSON.parse(String(runInit.body)).targets).toEqual(['target-1'])
    expect(screen.queryByRole('checkbox', { name: /Archive/ })).not.toBeInTheDocument()
  })

  it('requires an artifact file before it offers to restore', async () => {
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      return paged([])
    }))
    renderPage()
    await userEvent.click(await screen.findByRole('tab', { name: 'Backups' }))

    const restore = await screen.findByRole('button', { name: 'Restore' })
    expect(restore).toBeDisabled()

    const input = await screen.findByLabelText(/Backup artifact file/)
    fireEvent.change(input, { target: { files: [new File(['{"not":"an artifact"}'], 'notes.json', { type: 'application/json' })] } })
    expect(await screen.findByText('This file is not a restorable backup artifact.')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Restore' })).toBeDisabled()

    fireEvent.change(input, { target: { files: [new File([JSON.stringify(artifact)], 'backup.json', { type: 'application/json' })] } })
    await screen.findByText(/Artifact artifact-1234abcd/)
    expect(screen.getByRole('button', { name: 'Restore' })).toBeEnabled()
  })

  it('sends a resource-specific strategy beside the unchanged default', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      if (path.endsWith('/backup/restore')) return json({ restored: 2 })
      return paged([])
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    await userEvent.click(await screen.findByRole('tab', { name: 'Backups' }))
    fireEvent.change(await screen.findByLabelText(/Backup artifact file/), { target: { files: [new File([JSON.stringify(artifact)], 'backup.json', { type: 'application/json' })] } })
    await screen.findByText(/Artifact artifact-1234abcd/)

    await userEvent.click(screen.getByRole('combobox', { name: 'providers' }))
    await userEvent.click(await screen.findByRole('option', { name: 'Overwrite conflicts' }))
    await userEvent.click(screen.getByRole('button', { name: 'Restore' }))
    const confirm = await screen.findByRole('dialog')
    await userEvent.click(within(confirm).getByRole('button', { name: 'Restore' }))

    await waitFor(() => expect(fetchMock.mock.calls.some(([path]) => String(path).endsWith('/backup/restore'))).toBe(true))
    const [, init] = fetchMock.mock.calls.find(([path]) => String(path).endsWith('/backup/restore')) as [string, RequestInit]
    expect(JSON.parse(String(init.body))).toMatchObject({ strategy: 'fail', strategies: { providers: 'overwrite' } })
  })

  it('keeps the restore input intact when a delivery run is queued', async () => {
    const { clicked } = spyDownloads()
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      if (path.includes('/operations/storage')) return paged([{ id: 'target-1', name: 'Local backups', kind: 'local', enabled: true }])
      if (path.endsWith('/backup/run')) return json({ id: 'job-9' })
      if (init?.method === 'POST') return json({})
      return paged([])
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    await userEvent.click(await screen.findByRole('tab', { name: 'Backups' }))

    fireEvent.change(await screen.findByLabelText(/Backup artifact file/), { target: { files: [new File([JSON.stringify(artifact)], 'backup.json', { type: 'application/json' })] } })
    await screen.findByText(/Artifact artifact-1234abcd/)

    await userEvent.click(await screen.findByRole('checkbox', { name: /Local backups/ }))
    await userEvent.click(screen.getByRole('button', { name: 'Run now' }))

    // The run is queued as durable work and reports its own job id.
    await waitFor(() => expect(fetchMock.mock.calls.filter(([path]) => String(path).includes('/backup/run'))).toHaveLength(1))
    expect(vi.mocked(toast.success).mock.calls.map(([message]) => message)).toContain('Backup queued as job job-9')
    // The artifact the operator picked for restore is still there, untouched.
    expect(screen.getByText(/Artifact artifact-1234abcd/)).toBeInTheDocument()
    expect(clicked).toHaveLength(0)
  })

  it('shows per-target results and retries a run by queueing a new job', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      if (path.includes('/backup-target-results')) return paged([{ id: 'r1', job_id: 'j1', storage_id: 'target-1', status: 'failed', error_code: 'upstream_failed', byte_size: null }])
      if (path.includes('/backup/j1/retry')) return json({ id: 'job-2' })
      if (path.includes('/jobs')) return paged([job({ id: 'j1', status: 'failed', error_code: 'target_failed' })])
      if (path.includes('/operations/storage')) return paged([{ id: 'target-1', name: 'Local backups', kind: 'local', enabled: true }])
      if (init?.method === 'POST') return json({})
      return paged([])
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    await userEvent.click(await screen.findByRole('tab', { name: 'Backups' }))

    expect(await screen.findByText(/Local backups · Failed · — · upstream_failed/)).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: 'Retry' }))
    await waitFor(() => expect(fetchMock.mock.calls.filter(([path]) => String(path).includes('/backup/j1/retry'))).toHaveLength(1))
    expect(vi.mocked(toast.success).mock.calls.map(([message]) => message)).toContain('Retry queued as job job-2')
  })

  it('preflights an instance archive before it offers to replace the instance', async () => {
    const instanceArtifact = { version: 1, id: 'instance-1', created_at: 1_700_000_000, schema_digest: 'schema', database_digest: 'database', include_history: false, key_mode: 'master_key', salt: null, envelope: 'envelope' }
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      if (path.includes('/instance/restore/preflight')) return json({ artifact_id: 'instance-1', schema_version: 8, tables: { users: 3, projects: 1 }, destination_has_data: true, portable: false, active_sessions_restored: false, derived_available: false })
      if (init?.method === 'POST') return json({})
      return paged([])
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    await userEvent.click(await screen.findByRole('tab', { name: 'Backups' }))

    fireEvent.change(await screen.findByLabelText(/Instance backup file/), { target: { files: [new File([JSON.stringify(instanceArtifact)], 'instance.json', { type: 'application/json' })] } })
    await screen.findByText(/Artifact instance-1/)

    const replace = screen.getByRole('button', { name: 'Restore instance' })
    expect(replace).toBeDisabled()
    await userEvent.click(screen.getByRole('button', { name: 'Run preflight' }))

    const preflight = await screen.findByText('Preflight result')
    const panel = preflight.closest('.mantine-Paper-root') as HTMLElement
    expect(within(panel).getByText('Destination has data: Yes')).toBeInTheDocument()
    expect(within(panel).getByText(/Portable: No \(needs the original master key\)/)).toBeInTheDocument()
    expect(within(panel).getByText('The archive contains 2 tables')).toBeInTheDocument()
    expect(within(panel).getByText(/users 3/)).toBeInTheDocument()
    // The preflight is what unlocks the destructive action, and it warns about
    // the mode the operator picked.
    expect(within(panel).getByText(/fail mode will be refused/)).toBeInTheDocument()
    expect(fetchMock.mock.calls.filter(([path]) => String(path).includes('/instance/restore/preflight'))).toHaveLength(1)
  })

  it('hides the instance panel from a principal without the owner scope', async () => {
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['project:manage'])
      return paged([])
    }))
    renderPage(['project:manage'])
    await userEvent.click(await screen.findByRole('tab', { name: 'Backups' }))
    expect(await screen.findByRole('heading', { name: 'Backups' })).toBeInTheDocument()
    expect(screen.queryByText('Whole-instance backup')).not.toBeInTheDocument()
    expect(screen.queryByText('Instance backup file')).not.toBeInTheDocument()
  })
})

describe('instance diagnostics and proxy presets', () => {
  beforeEach(async () => { await i18n.changeLanguage('en'); vi.clearAllMocks() })

  it('exports bounded cache facts and confirms a clear', async () => {
    const diagnostic = { version: 1, generated_at: 1, runtime: { generation: 3, caches: [{ name: 'rotations', shape_version: 1, entries: 2, max_entries: 10000 }] } }
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      if (path.endsWith('/instance/diagnostics/cache')) return json(diagnostic)
      return paged([])
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    await userEvent.click(await screen.findByRole('tab', { name: 'Diagnostics' }))
    expect(await screen.findByText('rotations')).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: 'Clear cache' }))
    const dialog = await screen.findByRole('dialog')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Clear cache' }))
    await waitFor(() => expect(fetchMock.mock.calls.some(([path, init]) => String(path).endsWith('/instance/diagnostics/cache') && init?.method === 'DELETE')).toBe(true))
  })

  it('sends proxy credentials only on the write request', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      if (path.endsWith('/instance/proxy-presets')) return init?.method === 'POST' ? json({ id: 'proxy-1' }) : json({ data: [], total: 0 })
      return paged([])
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    await userEvent.click(await screen.findByRole('tab', { name: 'Outbound proxies' }))
    await userEvent.click(await screen.findByRole('button', { name: 'Add proxy' }))
    const dialog = await screen.findByRole('dialog')
    await userEvent.type(within(dialog).getByRole('textbox', { name: 'Name' }), 'Corporate egress')
    await userEvent.type(within(dialog).getByRole('textbox', { name: 'Proxy URL' }), 'https://proxy.example.test:8443')
    await userEvent.type(within(dialog).getByLabelText('Proxy username'), 'pangolin')
    await userEvent.type(within(dialog).getByLabelText('Proxy password'), 'sentinel')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Save' }))
    await waitFor(() => expect(fetchMock.mock.calls.some(([, init]) => init?.method === 'POST')).toBe(true))
    const [, init] = fetchMock.mock.calls.find(([path, init]) => String(path).endsWith('/instance/proxy-presets') && init?.method === 'POST') as [string, RequestInit]
    expect(JSON.parse(String(init.body))).toMatchObject({ name: 'Corporate egress', url: 'https://proxy.example.test:8443', credentials: { username: 'pangolin', password: 'sentinel' } })
  })
})

describe('jobs tab is a live list', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en')
    vi.clearAllMocks()
  })

  it('formats every timestamp and orders pending work by due time', async () => {
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      if (path.includes('/jobs')) return paged([
        job({ id: 'late', status: 'pending', due_at: 1_800_000_000, created_at: 1_700_000_000 }),
        job({ id: 'done', kind: 'probe', status: 'succeeded', due_at: 1_700_000_000, created_at: 1_750_000_000 }),
        job({ id: 'soon', status: 'pending', due_at: 1_700_000_500, created_at: 1_690_000_000 }),
      ])
      return paged([])
    }))
    renderPage()
    await userEvent.click(await screen.findByRole('tab', { name: 'Jobs' }))

    const expected = new Intl.DateTimeFormat('en', { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(1_800_000_000_000))
    expect(await screen.findAllByText(expected)).not.toHaveLength(0)
    // No raw Unix seconds anywhere in the table.
    expect(screen.queryByText('1800000000')).not.toBeInTheDocument()
    expect(screen.queryByText('1700000000')).not.toBeInTheDocument()

    const rows = screen.getAllByRole('row').slice(1)
    expect(within(rows[0]).getByText('Pending')).toBeInTheDocument()
    expect(within(rows[0]).getByText(new Intl.DateTimeFormat('en', { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(1_700_000_500_000)))).toBeInTheDocument()
    expect(within(rows[1]).getByText(new Intl.DateTimeFormat('en', { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(1_800_000_000_000)))).toBeInTheDocument()
  })

  it('filters by status and shows per-status counts from the API', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      if (path.includes('/jobs')) {
        // The count chips ask for one row per status and read the total.
        if (path.includes('limit=1')) return paged([], path.includes('q=pending') ? 4 : 0)
        return paged([job({ id: 'j1', status: 'pending' })], 5)
      }
      return paged([])
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    await userEvent.click(await screen.findByRole('tab', { name: 'Jobs' }))

    expect(await screen.findByRole('button', { name: 'Pending 4' })).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: 'Failed 0' }))
    await waitFor(() => expect(fetchMock.mock.calls.some(([path]) => String(path).includes('q=failed'))).toBe(true))
    expect(await screen.findByText('Failed')).toBeInTheDocument()
  })

  it('queues a brand-new durable job from a row instead of writing a status', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      if (path.endsWith('/operations/probe') && init?.method === 'POST') return json({ id: 'probe-job' })
      if (path.includes('/operations/channels')) return paged([{ id: 'c1', name: 'Primary', kind: 'openai', enabled: true }])
      if (path.includes('/jobs')) return paged([job({ id: 'j1', kind: 'probe', status: 'succeeded' })])
      return paged([])
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    await userEvent.click(await screen.findByRole('tab', { name: 'Jobs' }))

    await userEvent.click(await screen.findByRole('button', { name: 'Queue new job' }))
    const dialog = screen.getByRole('dialog', { name: 'Queue a new Probe job' })
    // The channel list is fetched when the dialog opens, and the first channel is the default.
    await within(dialog).findByText('Primary')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Queue new job' }))

    await waitFor(() => expect(fetchMock.mock.calls.filter(([path]) => String(path).endsWith('/operations/probe'))).toHaveLength(1))
    const [, init] = fetchMock.mock.calls.find(([path]) => String(path).endsWith('/operations/probe')) as [string, RequestInit]
    expect(JSON.parse(String(init.body))).toEqual({ provider_id: 'c1' })
    expect(vi.mocked(toast.success).mock.calls.map(([message]) => message)).toContain('Job queued')
    // Nothing here updates a job row: the only writes are new durable jobs.
    expect(fetchMock.mock.calls.filter(([, config]) => config?.method && config.method !== 'GET').map(([path]) => String(path))).toEqual(['/api/admin/v1/projects/p1/operations/probe'])
  })

  it('refreshes on demand with a live-refresh control and an updated-at line', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      if (path.includes('/jobs')) return paged([job({ id: 'j1', status: 'running' })])
      return paged([])
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    await userEvent.click(await screen.findByRole('tab', { name: 'Jobs' }))

    const before = fetchMock.mock.calls.filter(([path]) => String(path).includes('/jobs')).length
    await userEvent.click(await screen.findByRole('button', { name: 'Refresh' }))
    await waitFor(() => expect(fetchMock.mock.calls.filter(([path]) => String(path).includes('/jobs')).length).toBeGreaterThan(before))
    expect(screen.getByText(/Updated /)).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Auto refresh' })).toBeInTheDocument()
  })
})

describe('project and instance scope', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en')
    vi.clearAllMocks()
  })

  it('reaches the project-scoped tabs with project:manage alone', async () => {
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['project:manage'])
      return paged([])
    }))
    renderPage(['project:manage'])
    for (const name of ['Backups', 'Jobs', 'Storage', 'Webhooks', 'Retention', 'Orchestration settings']) {
      expect(await screen.findByRole('tab', { name })).toBeInTheDocument()
    }
    // Instance-scoped settings need the owner scope, so they are not offered.
    expect(screen.queryByRole('tab', { name: 'Request logging' })).not.toBeInTheDocument()

    await userEvent.click(screen.getByRole('tab', { name: 'Storage' }))
    expect(await screen.findByText('Project: Project A')).toBeInTheDocument()
  })

  it('shows the instance-scoped tab and the scope note for the owner', async () => {
    const writes: Array<Record<string, unknown>> = []
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      if (path.includes('/settings/request-logging')) {
        if (init?.method === 'PUT') { writes.push(JSON.parse(String(init.body))); return json({ ok: true }) }
        return json({ version: 1, enabled: false, default_level: 'off', key_override_enabled: false, key_disable_allowed: false, live_preview_enabled: true })
      }
      return paged([])
    }))
    renderPage()
    expect(await screen.findByRole('tab', { name: 'Request logging' })).toBeInTheDocument()
    await userEvent.click(screen.getByRole('tab', { name: 'Request logging' }))
    expect(await screen.findByRole('heading', { name: 'Request logging' })).toBeInTheDocument()
    const livePreview = await screen.findByRole('switch', { name: /Enable live request preview/ })
    expect(livePreview).toBeChecked()
    await userEvent.click(livePreview)
    await waitFor(() => expect(writes.at(-1)?.live_preview_enabled).toBe(false))
  })

  it('round-trips the request logging policy version', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.includes('/settings/request-logging') && init?.method === 'PUT') return json({ ok: true })
      if (path.includes('/settings/request-logging')) return json({ version: 1, enabled: false, default_level: 'off', key_override_enabled: false, key_disable_allowed: false })
      return paged([])
    })
    vi.stubGlobal('fetch', fetchMock)
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(
      <QueryClientProvider client={client}>
        <ThemeProvider><LoggingPolicy /></ThemeProvider>
      </QueryClientProvider>,
    )
    await userEvent.click(await screen.findByRole('switch', { name: /Enable request logging/ }))

    await waitFor(() => expect(fetchMock.mock.calls.some(([, init]) => init?.method === 'PUT')).toBe(true))
    const [, init] = fetchMock.mock.calls.find(([, request]) => request?.method === 'PUT') as [string, RequestInit]
    expect(JSON.parse(String(init.body))).toEqual({
      version: 1,
      enabled: true,
      default_level: 'off',
      key_override_enabled: false,
      key_disable_allowed: false,
    })
  })

  it('explains itself when neither scope is available', async () => {
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['catalog:manage'])
      return paged([])
    }))
    renderPage(['catalog:manage'])
    expect(await screen.findByText(/neither project nor instance administration/)).toBeInTheDocument()
    expect(screen.queryByRole('tab', { name: 'Backups' })).not.toBeInTheDocument()
    expect(screen.getByRole('tab', { name: 'About' })).toBeInTheDocument()
  })
})

/**
 * The orchestration panel is a real form over the project's stored document: the
 * GET fills it in and the PUT replaces the document. It lives in a
 * `keepMounted={false}` tab whose default is `appearance`, so a test that never
 * activates `Orchestration settings` mounts nothing at all — neither the routing
 * field nor its affinity/compaction siblings exist in the DOM. That is why the
 * earlier attempt concluded the routing field "could not render": it asserted
 * against an unmounted panel, not against the panel. These cases follow the
 * Backups pattern instead — render the page, click the tab, then assert.
 *
 * The guard is behavioural, not textual: it asserts the rendered value comes from
 * the stored document and that the submitted body carries the edited `routing`
 * verbatim. Delete the field, or drop `routing` from the submit body, and these
 * cases fail.
 */
describe('orchestration settings are a form over the stored document', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en')
    vi.clearAllMocks()
  })

  const orchestrationPath = '/api/admin/v1/projects/p1/settings/orchestration'
  /** A stored document with a nontrivial routing policy: the `{version:1}` fallback cannot stand in for it. */
  const stored = {
    version: 1,
    affinity_rules: [{ id: 'sticky-session', mode: 'prefer', source: { kind: 'header', value: 'x-session-id' }, ttl_secs: 600, release_on_failure: true }],
    session_compaction: { enabled: true, threshold_tokens: 4096, retain_items: 8, native: false, summarizer_model: 'gpt-4o-mini' },
    semantic_memory: { enabled: false, max_candidates: 4, rerank: false },
    routing: { version: 1, all: [{ field: '/body/model', op: 'regex', value: '^gpt-4' }, { field: '/headers/x-tier', op: 'eq', value: 'pro' }] },
  }

  const renderOrchestration = async (permissions: string[]) => {
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(permissions)
      if (path.endsWith('/settings/orchestration')) return json(stored)
      return paged([])
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage(permissions)
    await userEvent.click(await screen.findByRole('tab', { name: 'Orchestration settings' }))
    return fetchMock
  }
  const putBodies = (fetchMock: ReturnType<typeof vi.fn>) => fetchMock.mock.calls
    .filter(([, init]) => (init as RequestInit | undefined)?.method === 'PUT')
    .map(([path, init]) => ({ path: String(path), body: JSON.parse(String((init as RequestInit).body)) as Record<string, unknown> }))

  it('renders the stored routing document beside its siblings and submits the edit', async () => {
    const fetchMock = await renderOrchestration(['project:manage'])

    // All three fields exist because the tab is now mounted, and each one is
    // populated from the stored document rather than from a placeholder.
    const affinity = await screen.findByLabelText(/Affinity rules JSON/)
    const compaction = screen.getByLabelText(/Session compaction JSON/)
    const semanticMemory = screen.getByLabelText(/Semantic memory JSON/)
    const routing = screen.getByLabelText(/Routing policy/)
    expect(affinity).toHaveValue(JSON.stringify(stored.affinity_rules, null, 2))
    expect(compaction).toHaveValue(JSON.stringify(stored.session_compaction, null, 2))
    expect(semanticMemory).toHaveValue(JSON.stringify(stored.semantic_memory, null, 2))
    expect(routing).toHaveValue(JSON.stringify(stored.routing, null, 2))

    // A different, valid routing document: the operator's edit must be what is sent.
    const edited = { version: 1, any: [{ field: '/headers/x-tier', op: 'in', value: ['pro', 'scale'] }] }
    fireEvent.change(routing, { target: { value: JSON.stringify(edited, null, 2) } })
    await userEvent.click(screen.getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(putBodies(fetchMock)).toHaveLength(1))
    const [put] = putBodies(fetchMock)
    expect(put.path).toBe(orchestrationPath)
    // The whole document travels together: the routing policy alongside the
    // affinity rules and compaction settings the same form owns.
    expect(put.body).toEqual({
      version: 1,
      affinity_rules: stored.affinity_rules,
      session_compaction: stored.session_compaction,
      semantic_memory: stored.semantic_memory,
      routing: edited,
    })
    expect(vi.mocked(toast.success).mock.calls.map(([message]) => message)).toContain('Saved')
  })

  it('refuses invalid routing JSON locally and never issues the PUT', async () => {
    const fetchMock = await renderOrchestration(['project:manage'])
    const routing = await screen.findByLabelText(/Routing policy/)

    // The affinity and compaction documents stay valid, so the only parse that can
    // fail is the routing one.
    fireEvent.change(routing, { target: { value: '{"version": 1, "all": [{' } })
    await userEvent.click(screen.getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(toast.error).toHaveBeenCalledWith('The JSON is invalid.'))
    // Malformed JSON is the console's own feedback; anything the document *means*
    // is the backend's call, so no request is invented here.
    expect(putBodies(fetchMock)).toHaveLength(0)
    expect(toast.success).not.toHaveBeenCalled()
  })

  it('is not reachable, or rendered, without the project scope', async () => {
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['catalog:manage'])
      return paged([])
    }))
    renderPage(['catalog:manage'])
    expect(await screen.findByText(/neither project nor instance administration/)).toBeInTheDocument()
    expect(screen.queryByRole('tab', { name: 'Orchestration settings' })).not.toBeInTheDocument()
    expect(screen.queryByLabelText(/Routing policy/)).not.toBeInTheDocument()
  })
})

describe('model settings are an instance form', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en')
    vi.clearAllMocks()
  })

  const modelSettings = {
    version: 1,
    fallback_to_channels_on_model_not_found: true,
    query_all_channel_models: true,
    default_model_api_include_all: false,
    auto_reasoning_effort: false,
    model_blacklist_regex: '^private-',
    hide_unroutable_models_in_list: true,
  }

  const renderModelSettings = async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      if (path.endsWith('/settings/models') && init?.method === 'PUT') return json({ ok: true })
      if (path.endsWith('/settings/models')) return json(modelSettings)
      return paged([])
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    await userEvent.click(await screen.findByRole('tab', { name: 'Model settings' }, { timeout: 3_000 }))
    return fetchMock
  }

  it('loads every stored knob and submits one versioned document', async () => {
    const fetchMock = await renderModelSettings()

    expect(await screen.findByRole('heading', { name: 'Model settings' })).toBeInTheDocument()
    expect(screen.getByText('Applies to the whole Pangolin instance, independent of the selected project.')).toBeInTheDocument()
    expect(screen.getByRole('switch', { name: /^Fallback to channel models/ })).toBeChecked()
    expect(screen.getByRole('switch', { name: /^Query all channel models/ })).toBeChecked()
    expect(screen.getByRole('switch', { name: /^Include extended metadata by default/ })).not.toBeChecked()
    expect(screen.getByRole('switch', { name: /^Infer reasoning effort from model suffix/ })).not.toBeChecked()
    expect(screen.getByRole('switch', { name: /^Hide unroutable models/ })).toBeChecked()
    expect(screen.getByLabelText('Model blacklist regex')).toHaveValue('^private-')

    await userEvent.click(screen.getByRole('switch', { name: /^Include extended metadata by default/ }))
    await userEvent.click(screen.getByRole('switch', { name: /^Infer reasoning effort from model suffix/ }))
    await userEvent.clear(screen.getByLabelText('Model blacklist regex'))
    await userEvent.type(screen.getByLabelText('Model blacklist regex'), '^internal-(chat|embed)$')
    await userEvent.click(screen.getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(fetchMock.mock.calls.some(([, init]) => init?.method === 'PUT')).toBe(true))
    const [path, init] = fetchMock.mock.calls.find(([, request]) => request?.method === 'PUT') as [string, RequestInit]
    expect(path).toBe('/api/admin/v1/settings/models')
    expect(JSON.parse(String(init.body))).toEqual({
      ...modelSettings,
      default_model_api_include_all: true,
      auto_reasoning_effort: true,
      model_blacklist_regex: '^internal-(chat|embed)$',
    })
    expect(vi.mocked(toast.success)).toHaveBeenCalledWith('Saved')
  })

  it('shows inline regex validation and never sends an invalid document', async () => {
    const fetchMock = await renderModelSettings()
    const regex = await screen.findByLabelText('Model blacklist regex')
    await userEvent.clear(regex)
    fireEvent.change(regex, { target: { value: '[' } })
    await userEvent.click(screen.getByRole('button', { name: 'Save' }))

    expect(await screen.findByText('Enter a valid regular expression.')).toBeInTheDocument()
    expect(fetchMock.mock.calls.some(([, init]) => init?.method === 'PUT')).toBe(false)
  })

  it('offers retry after a load failure and stays hidden without owner scope', async () => {
    let attempts = 0
    const fetchMock = vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      if (path.endsWith('/settings/models')) {
        attempts += 1
        return attempts === 1 ? json({ error: { message: 'unavailable' } }, 503) : json(modelSettings)
      }
      return paged([])
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    await userEvent.click(await screen.findByRole('tab', { name: 'Model settings' }))
    await userEvent.click(await screen.findByRole('button', { name: 'Retry' }))
    expect(await screen.findByLabelText('Model blacklist regex')).toHaveValue('^private-')

    cleanup()
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['project:manage'])
      return paged([])
    }))
    renderPage(['project:manage'])
    expect(await screen.findByRole('tab', { name: 'Orchestration settings' })).toBeInTheDocument()
    expect(screen.queryByRole('tab', { name: 'Model settings' })).not.toBeInTheDocument()
  })
})
