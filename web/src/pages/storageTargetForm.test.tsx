import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { toast } from 'sonner'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import { ThemeProvider } from '../theme'
import SystemPage from './SystemPage'

/**
 * `src/operations/storage.rs::Config` is `deny_unknown_fields` and has exactly
 * two shapes: `{kind:'local',directory}` and
 * `{kind:'s3',endpoint,bucket,region,prefix?}`, with the S3 credentials in the
 * separately encrypted `secret`. The console asked for both as raw JSON
 * textareas, so an operator had to know that shape and any typo came back as
 * "invalid storage configuration". The form now has a field per accepted key
 * and keeps a JSON escape hatch for the credential envelope.
 */
vi.mock('sonner', () => ({ toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() } }))

const json = (value: unknown, status = 200) => Promise.resolve(new Response(JSON.stringify(value), { status, headers: { 'Content-Type': 'application/json' } }))
const paged = (rows: unknown[], total = rows.length) => json({ data: rows, total, offset: 0, limit: 25 })
const project = { id: 'p1', name: 'Project A', slug: 'project-a', owner_user_id: 'u1', is_default: true, enabled: true }
const archive = { id: 't2', name: 'Archive', kind: 's3', config: { kind: 's3', endpoint: 'https://s3.example.test', bucket: 'archives', region: 'eu-west-1', prefix: 'pangolin' }, revision: 3, enabled: true }

const renderPage = (targets: unknown[] = []) => {
  const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
    const path = String(input)
    if (path.endsWith('/projects')) return json([project])
    if (path.includes('/permissions')) return json(['*'])
    if (path.includes('/operations/storage')) return init?.method === 'POST' ? json({ id: 't9' }) : paged(targets)
    return paged([])
  })
  vi.stubGlobal('fetch', fetchMock)
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <ThemeProvider>
        <MemoryRouter>
          <ProjectProvider><SystemPage /></ProjectProvider>
        </MemoryRouter>
      </ThemeProvider>
    </QueryClientProvider>,
  )
  return fetchMock
}

const openStorageTab = async () => {
  await userEvent.click(await screen.findByRole('tab', { name: 'Storage' }))
  return await screen.findByRole('button', { name: 'Add storage' })
}

const posted = (fetchMock: ReturnType<typeof vi.fn>) => {
  const call = fetchMock.mock.calls.find(([, config]) => config?.method === 'POST' && String(config.body).includes('config'))
  return JSON.parse(String((call as [string, RequestInit])[1].body)) as Record<string, unknown>
}

describe('storage targets are a form, not a JSON box', () => {
  beforeEach(async () => { await i18n.changeLanguage('en'); vi.clearAllMocks() })

  it('posts a local target from named fields', async () => {
    const fetchMock = renderPage()
    await userEvent.click(await openStorageTab())
    const dialog = await screen.findByRole('dialog')
    await userEvent.type(within(dialog).getByRole('textbox', { name: 'Name' }), 'Local backups')
    await userEvent.type(within(dialog).getByLabelText('Local directory'), 'backups')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(fetchMock.mock.calls.some(([, config]) => config?.method === 'POST')).toBe(true))
    expect(posted(fetchMock)).toMatchObject({ name: 'Local backups', config: { kind: 'local', directory: 'backups' } })
  })

  it('posts an S3 target with its credentials in the encrypted envelope', async () => {
    const fetchMock = renderPage()
    await userEvent.click(await openStorageTab())
    const dialog = await screen.findByRole('dialog')
    await userEvent.type(within(dialog).getByRole('textbox', { name: 'Name' }), 'Archive')
    await userEvent.click(within(dialog).getByRole('combobox', { name: 'Type' }))
    await userEvent.click(await screen.findByRole('option', { name: 'S3-compatible' }))
    await userEvent.type(within(dialog).getByLabelText('Endpoint'), 'https://s3.example.test')
    await userEvent.type(within(dialog).getByLabelText('Bucket'), 'archives')
    await userEvent.type(within(dialog).getByLabelText('Region'), 'eu-west-1')
    await userEvent.type(within(dialog).getByLabelText('Access key ID'), 'AKIAEXAMPLE')
    await userEvent.type(within(dialog).getByLabelText('Secret access key'), 'super-secret')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(fetchMock.mock.calls.some(([, config]) => config?.method === 'POST')).toBe(true))
    expect(posted(fetchMock)).toMatchObject({
      config: { kind: 's3', endpoint: 'https://s3.example.test', bucket: 'archives', region: 'eu-west-1', prefix: '' },
      secret: { access_key_id: 'AKIAEXAMPLE', secret_access_key: 'super-secret' },
    })
  })

  it('posts GCS and WebDAV targets with credentials outside public config', async () => {
    const gcsFetch = renderPage()
    await userEvent.click(await openStorageTab())
    let dialog = await screen.findByRole('dialog')
    await userEvent.type(within(dialog).getByRole('textbox', { name: 'Name' }), 'GCS archive')
    await userEvent.click(within(dialog).getByRole('combobox', { name: 'Type' }))
    await userEvent.click(await screen.findByRole('option', { name: 'Google Cloud Storage' }))
    await userEvent.type(within(dialog).getByLabelText('Bucket'), 'pangolin-archive')
    fireEvent.change(within(dialog).getByLabelText('Service account JSON'), { target: { value: '{"client_email":"backup@example.test","private_key":"sentinel"}' } })
    await userEvent.click(within(dialog).getByRole('button', { name: 'Save' }))
    await waitFor(() => expect(gcsFetch.mock.calls.some(([, config]) => config?.method === 'POST')).toBe(true))
    expect(posted(gcsFetch)).toMatchObject({ config: { kind: 'gcs', bucket: 'pangolin-archive', prefix: '' }, secret: { service_account: { client_email: 'backup@example.test', private_key: 'sentinel' } } })

    cleanup()
    vi.clearAllMocks()
    const davFetch = renderPage()
    await userEvent.click(await openStorageTab())
    dialog = await screen.findByRole('dialog')
    await userEvent.type(within(dialog).getByRole('textbox', { name: 'Name' }), 'WebDAV archive')
    await userEvent.click(within(dialog).getByRole('combobox', { name: 'Type' }))
    await userEvent.click(await screen.findByRole('option', { name: 'WebDAV' }))
    await userEvent.type(within(dialog).getByLabelText('Endpoint'), 'https://dav.example.test/backups')
    await userEvent.type(within(dialog).getByLabelText('Username'), 'pangolin')
    await userEvent.type(within(dialog).getByLabelText('Password'), 'sentinel')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Save' }))
    await waitFor(() => expect(davFetch.mock.calls.some(([, config]) => config?.method === 'POST')).toBe(true))
    expect(posted(davFetch)).toMatchObject({ config: { kind: 'webdav', endpoint: 'https://dav.example.test/backups', prefix: '' }, secret: { username: 'pangolin', password: 'sentinel' } })
  })

  it('tests a stored target through the non-writing action', async () => {
    const fetchMock = renderPage([archive])
    await userEvent.click(await screen.findByRole('tab', { name: 'Storage' }))
    const row = await screen.findByRole('row', { name: /Archive/ })
    await userEvent.click(within(row).getByRole('button', { name: 'Test connection' }))
    await waitFor(() => expect(fetchMock.mock.calls.some(([path, config]) => String(path).endsWith('/operations/storage/t2/test') && config?.method === 'POST')).toBe(true))
    expect(toast.success).toHaveBeenCalledWith('Connection succeeded without writing an object.')
  })

  it('refuses a value the API would reject, and sends nothing', async () => {
    const fetchMock = renderPage()
    await userEvent.click(await openStorageTab())
    const dialog = await screen.findByRole('dialog')
    await userEvent.type(within(dialog).getByRole('textbox', { name: 'Name' }), 'Local backups')
    await userEvent.type(within(dialog).getByLabelText('Local directory'), '../etc')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    expect(vi.mocked(toast.error)).toHaveBeenCalledWith('The local directory must be 1–128 letters, digits, dashes or underscores.')
    expect(fetchMock.mock.calls.filter(([, config]) => config?.method === 'POST')).toHaveLength(0)
  })

  it('keeps a JSON escape hatch for a credential the fields do not cover', async () => {
    const fetchMock = renderPage()
    await userEvent.click(await openStorageTab())
    const dialog = await screen.findByRole('dialog')
    await userEvent.type(within(dialog).getByRole('textbox', { name: 'Name' }), 'Archive')
    await userEvent.click(within(dialog).getByRole('combobox', { name: 'Type' }))
    await userEvent.click(await screen.findByRole('option', { name: 'S3-compatible' }))
    await userEvent.type(within(dialog).getByLabelText('Endpoint'), 'https://s3.example.test')
    await userEvent.type(within(dialog).getByLabelText('Bucket'), 'archives')
    await userEvent.type(within(dialog).getByLabelText('Region'), 'eu-west-1')
    // `type` reads braces as key syntax, so the escape hatch is set directly.
    fireEvent.change(within(dialog).getByLabelText('Encrypted credentials JSON'), { target: { value: '{"access_key_id":"AKIA","secret_access_key":"s","session_token":"t"}' } })
    await userEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(fetchMock.mock.calls.some(([, config]) => config?.method === 'POST')).toBe(true))
    expect(posted(fetchMock)).toMatchObject({ secret: { access_key_id: 'AKIA', secret_access_key: 's', session_token: 't' } })
  })

  it('refuses a new S3 target without credentials, which could never deliver', async () => {
    const fetchMock = renderPage()
    await userEvent.click(await openStorageTab())
    const dialog = await screen.findByRole('dialog')
    await userEvent.type(within(dialog).getByRole('textbox', { name: 'Name' }), 'Archive')
    await userEvent.click(within(dialog).getByRole('combobox', { name: 'Type' }))
    await userEvent.click(await screen.findByRole('option', { name: 'S3-compatible' }))
    await userEvent.type(within(dialog).getByLabelText('Endpoint'), 'https://s3.example.test')
    await userEvent.type(within(dialog).getByLabelText('Bucket'), 'archives')
    await userEvent.type(within(dialog).getByLabelText('Region'), 'eu-west-1')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    expect(vi.mocked(toast.error)).toHaveBeenCalledWith('A new S3 target needs an access key ID and a secret access key.')
    expect(fetchMock.mock.calls.filter(([, config]) => config?.method === 'POST')).toHaveLength(0)
  })

  it('edits an S3 target from its stored config and leaves the stored envelope alone', async () => {
    const fetchMock = renderPage([archive])
    await userEvent.click(await screen.findByRole('tab', { name: 'Storage' }))
    const row = await screen.findByRole('row', { name: /Archive/ })
    // The list states the target in the terms the form uses, not as raw JSON.
    expect(within(row).getByText('archives @ https://s3.example.test · pangolin')).toBeInTheDocument()
    await userEvent.click(within(row).getByRole('button', { name: 'Edit' }))

    const dialog = await screen.findByRole('dialog')
    expect((within(dialog).getByLabelText('Endpoint') as HTMLInputElement).value).toBe('https://s3.example.test')
    expect((within(dialog).getByLabelText('Bucket') as HTMLInputElement).value).toBe('archives')
    expect((within(dialog).getByLabelText('Prefix') as HTMLInputElement).value).toBe('pangolin')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(fetchMock.mock.calls.some(([, config]) => config?.method === 'POST')).toBe(true))
    const body = posted(fetchMock)
    expect(body.config).toEqual({ kind: 's3', endpoint: 'https://s3.example.test', bucket: 'archives', region: 'eu-west-1', prefix: 'pangolin' })
    // The collection route upserts on the body's id, so an edit must carry it.
    expect(body.id).toBe('t2')
    // A blank credential field keeps the encrypted envelope the row already has.
    expect(body).not.toHaveProperty('secret')
  })
})
