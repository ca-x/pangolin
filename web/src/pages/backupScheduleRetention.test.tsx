import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { toast } from 'sonner'
import i18n from '../i18n'
import { ConfirmHost } from '../components'
import { ProjectProvider } from '../project'
import { ThemeProvider } from '../theme'
import SystemPage from './SystemPage'

/**
 * The retention count of an automatic-backup schedule is `payload.keep`
 * (`src/operations/runtime.rs` reads it, `src/operations/backup.rs::retain`
 * refuses anything outside 1–1000) and the storage target is
 * `payload.storage_id`. Both were reachable only by hand-writing the schedule's
 * payload JSON, so the count was effectively fixed at whatever the row was
 * created with.
 */
vi.mock('sonner', () => ({ toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() } }))

const json = (value: unknown, status = 200) => Promise.resolve(new Response(JSON.stringify(value), { status, headers: { 'Content-Type': 'application/json' } }))
const paged = (rows: unknown[], total = rows.length) => json({ data: rows, total, offset: 0, limit: 25 })
const project = { id: 'p1', name: 'Project A', slug: 'project-a', owner_user_id: 'u1', is_default: true, enabled: true }
const target = { id: 't1', name: 'Local backups', kind: 'local', config: { kind: 'local', directory: 'backups' }, revision: 1, enabled: true }
const schedule = { id: 's1', kind: 'backup_retention', payload: { storage_id: 't1', keep: 7 }, interval_secs: 86_400, next_run_at: 1_800_000_000, enabled: true, revision: 2, last_error: null }

const renderPage = (schedules: unknown[], targets: unknown[] = [target]) => {
  const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
    const path = String(input)
    if (path.endsWith('/projects')) return json([project])
    if (path.includes('/permissions')) return json(['*'])
    if (path.includes('/operations/schedules')) return init?.method === 'POST' ? json({ id: 's2' }) : paged(schedules)
    if (path.includes('/operations/storage')) return paged(targets)
    return paged([])
  })
  vi.stubGlobal('fetch', fetchMock)
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
  return fetchMock
}

describe('a backup schedule exposes its retention count', () => {
  beforeEach(async () => { await i18n.changeLanguage('en'); vi.clearAllMocks() })

  it('shows the stored target and count, and saves an edit into the payload', async () => {
    const fetchMock = renderPage([schedule])
    await userEvent.click(await screen.findByRole('tab', { name: 'Backups' }))

    const row = await screen.findByRole('row', { name: /backup_retention/ })
    await userEvent.click(within(row).getByRole('button', { name: 'Edit' }))

    const dialog = await screen.findByRole('dialog')
    // The count and the target come from the row's payload, not from a default:
    // editing must not silently reset them.
    const keep = within(dialog).getByLabelText('Retention count') as HTMLInputElement
    expect(keep.value).toBe('7')
    expect(within(dialog).getByRole('combobox', { name: 'Storage target' })).toHaveValue('Local backups')

    await userEvent.clear(keep)
    await userEvent.type(keep, '30')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(fetchMock.mock.calls.some(([, config]) => config?.method === 'POST')).toBe(true))
    const [, init] = fetchMock.mock.calls.find(([, config]) => config?.method === 'POST') as [string, RequestInit]
    const body = JSON.parse(String(init.body)) as Record<string, unknown>
    expect(body.payload).toEqual({ storage_id: 't1', keep: 30 })
    // The form's own controls are not sent as top-level fields the API ignores.
    expect(body).not.toHaveProperty('keep')
    expect(body).not.toHaveProperty('storage_id')
    expect(body.id).toBe('s1')
  })

  it('refuses a retention schedule with no storage target instead of queueing a job that cannot run', async () => {
    const fetchMock = renderPage([], [])
    await userEvent.click(await screen.findByRole('tab', { name: 'Backups' }))
    await userEvent.click(await screen.findByRole('button', { name: 'Add schedule' }))

    const dialog = await screen.findByRole('dialog')
    await userEvent.click(within(dialog).getByRole('combobox', { name: 'Type' }))
    await userEvent.click(await screen.findByRole('option', { name: 'Backup retention' }))
    await userEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    expect(vi.mocked(toast.error)).toHaveBeenCalledWith('Choose the storage target this retention schedule prunes.')
    expect(fetchMock.mock.calls.filter(([, config]) => config?.method === 'POST')).toHaveLength(0)
  })

  it('refuses a count the API would reject', async () => {
    const fetchMock = renderPage([schedule])
    await userEvent.click(await screen.findByRole('tab', { name: 'Backups' }))
    const row = await screen.findByRole('row', { name: /backup_retention/ })
    await userEvent.click(within(row).getByRole('button', { name: 'Edit' }))

    const dialog = await screen.findByRole('dialog')
    const keep = within(dialog).getByLabelText('Retention count')
    await userEvent.clear(keep)
    await userEvent.type(keep, '0')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    expect(vi.mocked(toast.error)).toHaveBeenCalledWith('The retention count must be a whole number between 1 and 1000.')
    expect(fetchMock.mock.calls.filter(([, config]) => config?.method === 'POST')).toHaveLength(0)
  })

  it('leaves the other kinds on their payload JSON', async () => {
    const automatic = { id: 's9', kind: 'automatic_backup', payload: { targets: ['t1'], resources: ['providers'] }, interval_secs: 3600, next_run_at: 1_800_000_000, enabled: true, revision: 1, last_error: null }
    const fetchMock = renderPage([automatic])
    await userEvent.click(await screen.findByRole('tab', { name: 'Backups' }))
    const row = await screen.findByRole('row', { name: /automatic_backup/ })
    await userEvent.click(within(row).getByRole('button', { name: 'Edit' }))

    const dialog = await screen.findByRole('dialog')
    // The retention inputs are blank here; the payload the row stores is intact.
    expect((within(dialog).getByLabelText('Configuration') as HTMLTextAreaElement).value).toContain('"targets"')
    expect((within(dialog).getByLabelText('Retention count') as HTMLInputElement).value).toBe('')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(fetchMock.mock.calls.some(([, config]) => config?.method === 'POST')).toBe(true))
    const [, init] = fetchMock.mock.calls.find(([, config]) => config?.method === 'POST') as [string, RequestInit]
    const body = JSON.parse(String(init.body)) as Record<string, unknown>
    expect(body).toMatchObject({ id: 's9', kind: 'automatic_backup', payload: { targets: ['t1'], resources: ['providers'] } })
    expect(body).not.toHaveProperty('keep')
  })
})

describe('a schedule that stopped running says why', () => {
  beforeEach(async () => { await i18n.changeLanguage('en'); vi.clearAllMocks() })

  // `jobs.rs` parks a schedule whose configuration the runner cannot use as
  // `last_error='invalid_configuration'` and stops advancing `next_run_at`, so it
  // will never fire again. The list rendered it exactly like a healthy one, which
  // is the console lying about status: the operator sees a next run that cannot
  // happen. The value is the record system's own code, shown verbatim.
  it('shows the recorded error for a schedule that will not run again', async () => {
    renderPage([{ ...schedule, last_error: 'invalid_configuration' }])
    await userEvent.click(await screen.findByRole('tab', { name: 'Backups' }))

    const row = await screen.findByRole('row', { name: /backup_retention/ })
    // The record's code, named in the active language (the zh-CN side is asserted
    // below); the verbatim code is the fallback for codes with no label.
    // The record says the last run failed; it does not say the schedule stopped —
    // a browser run proved this one kept firing every 30 seconds while the badge
    // claimed it had stopped.
    expect(within(row).getByText('Invalid configuration; the last run failed')).toBeInTheDocument()
  })

  it('shows — rather than a fabricated state for a schedule with no error', async () => {
    renderPage([schedule])
    await userEvent.click(await screen.findByRole('tab', { name: 'Backups' }))

    const row = await screen.findByRole('row', { name: /backup_retention/ })
    expect(within(row).getByText('—')).toBeInTheDocument()
    expect(within(row).queryByText('invalid_configuration')).not.toBeInTheDocument()
  })
})

describe('a recorded error code is copy the operator can read', () => {
  beforeEach(async () => { await i18n.changeLanguage('zh-CN'); vi.clearAllMocks() })

  // `invalid_configuration`, `attempts_exhausted` and `operation_failed` are the
  // record's machine codes. Showing them verbatim in a Chinese console is honest
  // but it is not copy: AGENTS.md asks for localized labels, and the console
  // already maps codes this way for error kinds.
  it('names a known code in the active language', async () => {
    renderPage([{ ...schedule, last_error: 'invalid_configuration' }])
    await userEvent.click(await screen.findByRole('tab', { name: '备份' }))

    const row = await screen.findByRole('row', { name: /backup_retention/ })
    expect(within(row).queryByText('invalid_configuration')).not.toBeInTheDocument()
    expect(within(row).getByText(/配置/)).toBeInTheDocument()
  })

  it('still shows a code it has no label for, rather than hiding it', async () => {
    renderPage([{ ...schedule, last_error: 'some_future_code' }])
    await userEvent.click(await screen.findByRole('tab', { name: '备份' }))

    const row = await screen.findByRole('row', { name: /backup_retention/ })
    expect(within(row).getByText('some_future_code')).toBeInTheDocument()
  })
})

describe('a schedule interval the server would refuse is refused locally, in the operator language', () => {
  beforeEach(async () => { await i18n.changeLanguage('en'); vi.clearAllMocks() })

  // The server accepts 30 s to 365 days and answers anything else with a bare
  // English "invalid schedule interval"; the form stated no rule and passed that
  // sentence through. The bounds are the server's own.
  it('states the bounds and refuses an interval outside them without asking the server', async () => {
    const fetchMock = renderPage([])
    await userEvent.click(await screen.findByRole('tab', { name: 'Backups' }))
    await userEvent.click(await screen.findByRole('button', { name: 'Add schedule' }))

    const dialog = await screen.findByRole('dialog')
    const interval = within(dialog).getByLabelText('Interval seconds') as HTMLInputElement
    await userEvent.clear(interval)
    await userEvent.type(interval, '5')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    expect(await within(dialog).findByText(/30/)).toBeInTheDocument()
    const posted = fetchMock.mock.calls.filter(([input, init]) => String(input).includes('/operations/schedules') && (init as RequestInit | undefined)?.method === 'POST')
    expect(posted).toHaveLength(0)
  })
})

describe('a daily backup schedule keeps its wall time and timezone', () => {
  beforeEach(async () => { await i18n.changeLanguage('en'); vi.clearAllMocks() })

  it('submits an anchored daily schedule without replacing interval schedules', async () => {
    const fetchMock = renderPage([])
    await userEvent.click(await screen.findByRole('tab', { name: 'Backups' }))
    await userEvent.click(await screen.findByRole('button', { name: 'Add schedule' }))

    const dialog = await screen.findByRole('dialog')
    await userEvent.click(within(dialog).getByRole('combobox', { name: 'Schedule mode' }))
    await userEvent.click(await screen.findByRole('option', { name: 'Daily time' }))
    await userEvent.clear(within(dialog).getByLabelText('Daily time'))
    await userEvent.type(within(dialog).getByLabelText('Daily time'), '02:00')
    await userEvent.clear(within(dialog).getByLabelText('Timezone'))
    await userEvent.type(within(dialog).getByLabelText('Timezone'), 'America/New_York')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(fetchMock.mock.calls.some(([, config]) => config?.method === 'POST')).toBe(true))
    const [, init] = fetchMock.mock.calls.find(([, config]) => config?.method === 'POST') as [string, RequestInit]
    const body = JSON.parse(String(init.body))
    expect(body.payload.schedule).toEqual({ type: 'daily', time: '02:00', timezone: 'America/New_York' })
  })
})
