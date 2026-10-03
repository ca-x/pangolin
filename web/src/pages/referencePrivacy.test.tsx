import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, expect, it, vi } from 'vitest'
import { MemoryRouter } from 'react-router'
import i18n from '../i18n'
import { ProjectProvider, useProject } from '../project'
import PromptsPage from './PromptsPage'
import { json, mockApi } from './referenceConsoleFixtures'
function ProjectSwitches() {
  const { setProjectId } = useProject()
  return (
    <>
      <button onClick={() => setProjectId('p1')}>Project A</button>
      <button onClick={() => setProjectId('p2')}>Project B</button>
    </>
  )
}
function mountPrivacy(fetchMock: ReturnType<typeof mockApi>) {
  vi.stubGlobal('fetch', fetchMock)
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const view = render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={['/prompts?tab=protection']}>
        <ProjectProvider>
          <ProjectSwitches />
          <PromptsPage />
        </ProjectProvider>
      </MemoryRouter>
    </QueryClientProvider>,
  )
  return { client, ...view }
}
beforeEach(async () => {
  localStorage.clear()
  await i18n.changeLanguage('en')
})
const result = {
  decision: 'redact',
  redacted_body: { input: 'redacted' },
  findings: [{ rule_id: 'r1', path: '/input', start: 0, end: 5, action: 'redact', reason: 'matched' }],
  suppressed_findings: [{ rule_id: 'r2', path: '/input', start: 6, end: 9, action: 'redact', reason: 'allowlisted' }],
  truncated: false,
}
async function openStructured() {
  await userEvent.click(await screen.findByRole('button', { name: 'Preview rules' }))
  await userEvent.click(screen.getByRole('tab', { name: 'Full request' }))
  return screen.getByRole('textbox', { name: 'JSON request' })
}
it('posts endpoint/body only and explains structured decisions, paths, suppression and offsets', async () => {
  const fetchMock = mockApi((path) => (path.endsWith('/protection-request-preview') ? json(result) : undefined))
  mountPrivacy(fetchMock)
  const input = await openStructured()
  await userEvent.click(screen.getByRole('combobox', { name: 'Protocol endpoint' }))
  await userEvent.click(await screen.findByRole('option', { name: 'Responses' }))
  fireEvent.change(input, { target: { value: '{"input":"sample"}' } })
  await userEvent.click(screen.getByRole('button', { name: 'Run preview' }))
  expect(await screen.findByText(/Exact-match allowlist/)).toBeInTheDocument()
  expect(screen.getByText('Byte range: 0–5')).toBeInTheDocument()
  expect(screen.getAllByText('/input')).toHaveLength(2)
  const call = fetchMock.mock.calls.find(([path]) => String(path).endsWith('/protection-request-preview'))!
  expect(JSON.parse(String(call[1]?.body))).toEqual({ endpoint: '/v1/responses', body: { input: 'sample' } })
  expect(screen.getByLabelText('Resulting request')).toHaveTextContent('redacted')
})
it('blocks malformed/oversize JSON locally and keeps legacy text preview accessible', async () => {
  const fetchMock = mockApi()
  mountPrivacy(fetchMock)
  const input = await openStructured()
  fireEvent.change(input, { target: { value: '{bad' } })
  expect(screen.getByText('Enter a JSON object.')).toBeInTheDocument()
  expect(screen.getByRole('button', { name: 'Run preview' })).toBeDisabled()
  fireEvent.change(input, { target: { value: JSON.stringify({ input: 'x'.repeat(65536) }) } })
  expect(screen.getByText('The JSON request must not exceed 64 KiB.')).toBeInTheDocument()
  expect(fetchMock.mock.calls.some(([path]) => String(path).endsWith('/protection-request-preview'))).toBe(false)
  await userEvent.click(screen.getByRole('tab', { name: 'Text sample' }))
  expect(screen.getByRole('textbox', { name: 'Sample text' })).toBeInTheDocument()
})
it('disposes pending close, A→B→A and replacement generations without retaining samples in query/mutation caches', async () => {
  const finish: Array<(value: Response) => void> = []
  const fetchMock = mockApi((path) =>
    path.endsWith('/protection-request-preview') ? new Promise<Response>((resolve) => finish.push(resolve)) : undefined,
  )
  const { client } = mountPrivacy(fetchMock)
  let input = await openStructured()
  fireEvent.change(input, { target: { value: '{"input":"private sample"}' } })
  await userEvent.click(screen.getByRole('button', { name: 'Run preview' }))
  await userEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Close' }))
  input = await openStructured()
  expect(input).toHaveValue('')
  finish[0](await json(result))
  await waitFor(() => expect(screen.queryByLabelText('Resulting request')).not.toBeInTheDocument())
  fireEvent.change(input, { target: { value: '{"input":"private older"}' } })
  await userEvent.click(screen.getByRole('button', { name: 'Run preview' }))
  await userEvent.click(screen.getByRole('button', { name: 'Project B' }))
  await userEvent.click(screen.getByRole('button', { name: 'Project A' }))
  input = await openStructured()
  expect(input).toHaveValue('')
  finish[1](await json(result))
  await waitFor(() => expect(screen.queryByLabelText('Resulting request')).not.toBeInTheDocument())
  fireEvent.change(input, { target: { value: '{"input":"private replacement one"}' } })
  await userEvent.click(screen.getByRole('button', { name: 'Run preview' }))
  fireEvent.change(input, { target: { value: '{"input":"private replacement two"}' } })
  await userEvent.click(screen.getByRole('button', { name: 'Run preview' }))
  finish[3](await json({ ...result, redacted_body: { input: 'new result' } }))
  expect(await screen.findByLabelText('Resulting request')).toHaveTextContent('new result')
  finish[2](await json(result))
  await waitFor(() => expect(screen.getByLabelText('Resulting request')).toHaveTextContent('new result'))
  expect(
    JSON.stringify(
      client
        .getQueryCache()
        .getAll()
        .map((q) => q.state.data),
    ),
  ).not.toContain('private')
  expect(
    JSON.stringify(
      client
        .getMutationCache()
        .getAll()
        .map((m) => m.state),
    ),
  ).not.toContain('private')
  expect(localStorage.getItem('private sample')).toBeNull()
})

it('templates remain opt-in, save through audited protection mutation and validate exact allowlists inline', async () => {
  const templates = [
    {
      template_id: 'email',
      name: 'Email',
      content_pattern: 'EMAIL_PATTERN',
      action: 'redact',
      replacement: '[REDACTED]',
      enabled: false,
      allowlist: [],
    },
  ]
  const fetchMock = mockApi((path, init) =>
    path.includes('/protection-templates')
      ? json({ templates })
      : path.includes('/operations/protection') && init?.method !== 'POST'
        ? json({
            data: [
              {
                id: 'r1',
                name: 'Existing rule',
                content_pattern: 'EMAIL_PATTERN',
                action: 'redact',
                enabled: true,
                state: 'active',
                allowlist: ['allowed'],
              },
            ],
            total: 1,
          })
        : undefined,
  )
  mountPrivacy(fetchMock)
  await screen.findAllByText('Existing rule')
  await userEvent.click(screen.getByRole('button', { name: 'Rule templates' }))
  await userEvent.click(await screen.findByRole('combobox', { name: 'Template' }))
  await userEvent.click(await screen.findByRole('option', { name: 'Email addresses' }))
  expect(fetchMock.mock.calls.some(([, init]) => init?.method === 'POST')).toBe(false)
  const dialog = within(screen.getByRole('dialog'))
  const list = dialog.getByRole('textbox', { name: 'Exact-match allowlist' })
  fireEvent.change(list, { target: { value: '[42]' } })
  expect(dialog.getByText('Enter up to 64 strings, each no more than 256 UTF-8 bytes.')).toBeInTheDocument()
  expect(dialog.getByRole('button', { name: 'Save template as rule' })).toBeDisabled()
  fireEvent.change(list, { target: { value: '["allowed"]' } })
  await userEvent.click(dialog.getByRole('button', { name: 'Save template as rule' }))
  await waitFor(() =>
    expect(
      fetchMock.mock.calls.some(
        ([path, init]) =>
          String(path).endsWith('/p1/operations/protection') &&
          JSON.parse(String(init?.body ?? 'null'))?.enabled === false,
      ),
    ).toBe(true),
  )
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
  await userEvent.click(screen.getAllByRole('button', { name: 'Edit' })[0])
  const editor = within(screen.getByRole('dialog'))
  fireEvent.change(editor.getByRole('textbox', { name: 'Exact-match allowlist' }), {
    target: { value: JSON.stringify(['汉'.repeat(86)]) },
  })
  expect(editor.getByRole('button', { name: 'Save' })).toBeDisabled()
})
it.each(['en', 'zh-CN'])('shows request-preview help, errors and retry in %s', async (language) => {
  await i18n.changeLanguage(language)
  let failed = true
  const fetchMock = mockApi((path) =>
    path.endsWith('/protection-request-preview')
      ? json(failed ? { error: { message: 'safe failure' } } : result, failed ? 503 : 200)
      : undefined,
  )
  mountPrivacy(fetchMock)
  await userEvent.click(await screen.findByRole('button', { name: i18n.t('previewRules') }))
  await userEvent.click(screen.getByRole('tab', { name: i18n.t('privacyRequest') }))
  expect(screen.getByText(i18n.t('privacyContextHelp'))).toBeInTheDocument()
  fireEvent.change(screen.getByRole('textbox', { name: i18n.t('privacyJson') }), {
    target: { value: '{"input":"sample"}' },
  })
  await userEvent.click(screen.getByRole('button', { name: i18n.t('runPreview') }))
  expect(await screen.findByText(i18n.t('privacyFailure'))).toBeInTheDocument()
  failed = false
  await userEvent.click(screen.getByRole('button', { name: i18n.t('retry') }))
  expect(await screen.findByLabelText(i18n.t('privacyResult'))).toHaveTextContent('redacted')
})
it('shows template lookup error/retry and useful empty state without silently saving a rule', async () => {
  let failed = true
  const fetchMock = mockApi((path) =>
    path.includes('/protection-templates')
      ? json(failed ? { error: { message: 'safe error' } } : { templates: [] }, failed ? 503 : 200)
      : undefined,
  )
  mountPrivacy(fetchMock)
  await userEvent.click(await screen.findByRole('button', { name: i18n.t('protectionTemplates') }))
  const dialog = within(screen.getByRole('dialog'))
  await dialog.findByRole('button', { name: i18n.t('retry') })
  failed = false
  await userEvent.click(dialog.getByRole('button', { name: i18n.t('retry') }))
  expect(await dialog.findByText(i18n.t('templateEmpty'))).toBeInTheDocument()
  expect(fetchMock.mock.calls.some(([, init]) => init?.method === 'POST')).toBe(false)
})
it('keeps legacy text preview usable and disposes text samples/results and pending responses on close', async () => {
  let finish: ((value: Response) => void) | undefined
  let deferred = false
  const response = {
    rules: [
      {
        id: 'text-rule',
        name: 'Text rule',
        description: '',
        action: 'redact',
        enabled: true,
        state: 'active',
        matched: true,
        result: '[REDACTED]',
      },
    ],
  }
  const fetchMock = mockApi((path) =>
    path.endsWith('/protection-preview')
      ? deferred
        ? new Promise<Response>((resolve) => {
            finish = resolve
          })
        : json(response)
      : undefined,
  )
  const { client } = mountPrivacy(fetchMock)
  await userEvent.click(await screen.findByRole('button', { name: i18n.t('previewRules') }))
  let input = screen.getByRole('textbox', { name: i18n.t('sampleText') })
  fireEvent.change(input, { target: { value: 'private legacy sample' } })
  await userEvent.click(screen.getByRole('button', { name: i18n.t('runPreview') }))
  expect(await screen.findByText('[REDACTED]')).toBeInTheDocument()
  const call = fetchMock.mock.calls.find(([path]) => String(path).endsWith('/protection-preview'))!
  expect(JSON.parse(String(call[1]?.body))).toEqual({ text: 'private legacy sample' })
  await userEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: i18n.t('close') }))
  deferred = true
  await userEvent.click(screen.getByRole('button', { name: i18n.t('previewRules') }))
  input = screen.getByRole('textbox', { name: i18n.t('sampleText') })
  expect(input).toHaveValue('')
  expect(screen.queryByText('[REDACTED]')).not.toBeInTheDocument()
  fireEvent.change(input, { target: { value: 'private legacy pending' } })
  await userEvent.click(screen.getByRole('button', { name: i18n.t('runPreview') }))
  await userEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: i18n.t('close') }))
  await userEvent.click(screen.getByRole('button', { name: i18n.t('previewRules') }))
  finish!(await json(response))
  await waitFor(() => expect(screen.queryByText('[REDACTED]')).not.toBeInTheDocument())
  expect(screen.getByRole('textbox', { name: i18n.t('sampleText') })).toHaveValue('')
  expect(
    JSON.stringify(
      client
        .getQueryCache()
        .getAll()
        .map((q) => q.state.data),
    ),
  ).not.toContain('private legacy')
  expect(
    JSON.stringify(
      client
        .getMutationCache()
        .getAll()
        .map((m) => m.state),
    ),
  ).not.toContain('private legacy')
})
