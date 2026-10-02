import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, expect, it } from 'vitest'
import i18n from '../i18n'
import ChannelsPage from './ChannelsPage'
import { json, mockApi, models, mount } from './referenceConsoleFixtures'

beforeEach(async () => { localStorage.clear(); await i18n.changeLanguage('en') })
const choose = async (label: string, option: string) => { await userEvent.click(screen.getByRole('combobox', { name: label })); await userEvent.click(await screen.findByRole('option', { name: option })) }
it('posts chosen credential, protocol and stream and resets dependent choices across channels/projects', async () => {
  const fetchMock = mockApi()
  mount(<ChannelsPage />, '/channels?tab=probes', fetchMock)
  await screen.findByRole('combobox', { name: 'Probe protocol' })
  await choose('Probe credential', '•••• 2222')
  await choose('Probe protocol', 'Responses')
  await userEvent.click(screen.getByRole('switch', { name: 'Stream' }))
  await userEvent.click(screen.getByRole('button', { name: 'Run probe' }))
  await waitFor(() => expect(fetchMock.mock.calls.some(([, init]) => init?.body === JSON.stringify({ provider_id: 'c1', model_id: 'm1', credential_id: 'k2', endpoint: '/v1/responses', stream: true }))).toBe(true))
  await userEvent.click(screen.getByRole('button', { name: 'Switch project' }))
  await screen.findByDisplayValue('Gemini model')
  expect(screen.getByRole('switch', { name: 'Stream' })).not.toBeChecked()
  await userEvent.click(screen.getByRole('switch', { name: 'Stream' }))
  await userEvent.click(screen.getByRole('button', { name: 'Run probe' }))
  await waitFor(() => expect(fetchMock.mock.calls.some(([path, init]) => String(path).includes('/p2/') && init?.body === JSON.stringify({ provider_id: 'c2', model_id: 'm2', credential_id: 'k3', endpoint: '/v1beta/models:streamGenerateContent', stream: true }))).toBe(true))
})
it.each(['en', 'zh-CN'])('shows honest phase measurements and unmeasured rates in %s', async (language) => {
  await i18n.changeLanguage(language)
  const rows = [
    { id: 'old', provider_id: 'c1', model: 'legacy headers', success: true, response_headers_ms: 12, first_event_ms: null, first_text_ms: null, ttft_ms: null, output_tokens: null, latency_ms: 100, stream: null },
    { id: 'nonstream', provider_id: 'c1', model: 'nonstream zero', success: true, response_headers_ms: 0, first_event_ms: null, first_text_ms: null, ttft_ms: 10, output_tokens: 0, latency_ms: 100, stream: false },
    { id: 'stream', provider_id: 'c1', model: 'measured zero', success: true, response_headers_ms: 0, first_event_ms: 5, first_text_ms: 10, output_tokens: 0, latency_ms: 100, stream: true },
  ]
  mount(<ChannelsPage />, '/channels?tab=probes', mockApi((path) => path.includes('/operations/probes') ? json({ data: rows, total: 3 }) : undefined))
  const region = within(await screen.findByRole('region', { name: i18n.t('probes') }))
  expect(region.getByText(i18n.t('probeResponseHeaders'))).toBeInTheDocument()
  expect(region.getByText(i18n.t('probeFirstEvent'))).toBeInTheDocument()
  expect(region.getByText(i18n.t('probeFirstText'))).toBeInTheDocument()
  expect(within(document.querySelector('.mobile-resource-list') as HTMLElement).getAllByText(i18n.t('probeFirstText')).length).toBeGreaterThan(0)
  const old = within(region.getByText('legacy headers').closest('tr')!)
  expect(old.getByText('12 ms')).toBeInTheDocument()
  expect(old.getAllByText('—').length).toBeGreaterThan(2)
  const nonstream = within(region.getByText('nonstream zero').closest('tr')!)
  expect(nonstream.queryByText('0.0')).not.toBeInTheDocument()
  expect(within(region.getByText('measured zero').closest('tr')!).getByText('0.0')).toBeInTheDocument()
})
it('blocks selection when credential lookup fails and retries to an explicit empty state', async () => {
  let failed = true
  mount(<ChannelsPage />, '/channels?tab=probes', mockApi((path) => path.includes('/operations/credentials') ? json(failed ? { error: { message: 'failed' } } : { data: [], total: 0 }, failed ? 503 : 200) : undefined))
  expect(await screen.findByText('Probe credentials could not be loaded.')).toBeInTheDocument()
  expect(screen.getByRole('button', { name: 'Run probe' })).toBeDisabled()
  failed = false
  await userEvent.click(screen.getByRole('button', { name: 'Retry' }))
  expect(await screen.findByText('No enabled credentials for this channel.')).toBeInTheDocument()
  expect(screen.getByRole('button', { name: 'Run probe' })).toBeDisabled()
})
it('resets protocol/credential/stream on channel change and excludes disabled credentials', async () => {
  const fetchMock = mockApi()
  mount(<ChannelsPage />, '/channels?tab=probes', fetchMock)
  await screen.findByRole('combobox', { name: 'Probe credential' })
  await userEvent.click(screen.getByRole('combobox', { name: 'Probe credential' }))
  expect(screen.queryByRole('option', { name: '•••• 9999' })).not.toBeInTheDocument()
  await userEvent.click(await screen.findByRole('option', { name: '•••• 2222' }))
  await choose('Probe protocol', 'Responses')
  await userEvent.click(screen.getByRole('switch', { name: 'Stream' }))
  await choose('Channel', 'Gemini')
  await screen.findByDisplayValue('Gemini model')
  expect(screen.getByRole('combobox', { name: 'Probe credential' })).toHaveValue('•••• 3333')
  expect(screen.getByRole('switch', { name: 'Stream' })).not.toBeChecked()
  await userEvent.click(screen.getByRole('button', { name: 'Run probe' }))
  await waitFor(() => expect(fetchMock.mock.calls.some(([, init]) => init?.body === JSON.stringify({ provider_id: 'c2', model_id: 'm2', credential_id: 'k3', endpoint: '/v1beta/models:generateContent', stream: false }))).toBe(true))
})
it('filters probe history server-side and retries a failed history query', async () => {
  let failed = true
  const fetchMock = mockApi((path) => path.includes('/operations/probes') ? json(failed ? { error: { message: 'history failed' } } : { data: [], total: 0 }, failed ? 503 : 200) : undefined)
  mount(<ChannelsPage />, '/channels?tab=probes', fetchMock)
  await screen.findByText(i18n.t('networkError'))
  failed = false
  await userEvent.click(screen.getByRole('button', { name: 'Retry' }))
  await screen.findByText(i18n.t('probeEmpty'))
  await choose('History channel', 'Primary')
  await waitFor(() => expect(fetchMock.mock.calls.some(([path]) => String(path).includes('operations/probes?') && String(path).includes('provider_id=c1'))).toBe(true))
})
it('shows loading until credential options resolve and honors a channel streaming prohibition', async () => {
  let resolve!: (response: Response) => void
  const pending = new Promise<Response>((done) => { resolve = done })
  mount(<ChannelsPage />, '/channels?tab=probes', mockApi((path) => {
    if (path.includes('/operations/credentials')) return pending
    if (path.includes('/operations/channel-settings')) return json({ data: [{ id: 'c1', provider_id: 'c1', model_rules: { version: 1, stream: false } }], total: 1 })
  }))
  expect(await screen.findByRole('button', { name: 'Run probe' })).toBeDisabled()
  expect(screen.getByRole('status')).toHaveTextContent(i18n.t('loading'))
  resolve(await json({ data: [{ id: 'k1', provider_id: 'c1', suffix: '1111', enabled: true }], total: 1 }))
  await waitFor(() => expect(screen.getByRole('button', { name: 'Run probe' })).toBeEnabled())
  expect(screen.getByRole('switch', { name: 'Stream' })).toBeDisabled()
})

it('keeps a known unsupported model stream disabled while allowing a nonstream probe', async () => {
  mount(<ChannelsPage />, '/channels?tab=probes', mockApi((path) => path.includes('/operations/models') ? json({ data: [{ ...models[0], catalog_metadata: { capabilities: { streaming: false } } }], total: 1 }) : undefined))
  await waitFor(() => expect(screen.getByRole('button', { name: 'Run probe' })).toBeEnabled())
  expect(screen.getByRole('switch', { name: 'Stream' })).toBeDisabled()
})
