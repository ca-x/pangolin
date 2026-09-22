import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { CapabilityTags } from '../components'
import { ProjectProvider } from '../project'
import AccessPage from './AccessPage'
import ChannelsPage from './ChannelsPage'

/**
 * The zh-CN console still shipped English inside itself: Mantine's password
 * toggle kept its own `aria-label`, sonner's region kept "Notifications alt+T",
 * and the record system's enums — channel kind, credential type, API-key type
 * and capability names — were printed verbatim in a table of Chinese labels.
 * An enum the console does not know stays verbatim: it is data, not copy.
 */
const json = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }))
const project = { id: 'p1', name: 'Project', slug: 'project', owner_user_id: 'u1', is_default: true, enabled: true }

const mockApi = (rows: unknown[], keyRows: unknown[] = []) => vi.fn((input: RequestInfo | URL) => {
  const path = String(input)
  if (path.includes('/api/v1/bootstrap')) return json({ initialized: true, authenticated: true, user: { id: 'u1', email: 'owner@example.test', role: 'admin', language: 'zh-CN', theme: 'system:bronze', created_at: 1 }, capture_payloads: false, branding: { instance_name: 'Pangolin', branding_name: 'Pangolin', favicon_url: '/logo.webp', onboarding_complete: true } })
  if (path.includes('settings/request-logging')) return json({ enabled: true, default_level: 'metadata', key_override_enabled: false, key_disable_allowed: false })
  if (path.endsWith('/projects')) return json([project])
  if (path.includes('/permissions')) return json(['*'])
  if (path.includes('/api-keys')) return json(keyRows)
  if (path.includes('/operations/')) return json({ data: rows, total: rows.length })
  return json({ data: [], total: 0 })
})

const renderPage = (page: React.ReactElement) => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider>{page}</ProjectProvider></MemoryRouter></QueryClientProvider>)
}

describe('the zh-CN console has no English left in it', () => {
  beforeEach(() => { void i18n.changeLanguage('zh-CN') })

  it('names the password visibility toggle in the active language', async () => {
    const { Login } = await import('../Auth')
    render(<MemoryRouter><Login /></MemoryRouter>)
    const toggle = screen.getByLabelText(i18n.t('togglePasswordVisibility', { lng: 'zh-CN' }))
    expect(toggle).toBeInTheDocument()
    expect(screen.queryByLabelText('Toggle password visibility')).not.toBeInTheDocument()
  })

  it('routes every password field through that one named toggle', () => {
    // Mantine's PasswordInput names its toggle in English whatever the console's
    // language is, so no page may use it directly.
    for (const file of ['Auth.tsx', 'pages/AccessPage.tsx', 'pages/SystemPage.tsx', 'pages/PlaygroundPage.tsx', 'pages/shared.tsx']) {
      expect([file, /<PasswordInput/.test(readFileSync(join(process.cwd(), 'src', file), 'utf8'))]).toEqual([file, false])
    }
  })

  it('translates the channel kind and the credential type', async () => {
    const channels = [{ id: 'c1', name: 'QA Mock Upstream', kind: 'openai_compatible', base_url: 'http://127.0.0.1:18900/v1', enabled: true }]
    const credentials = [{ id: 'k1', provider_id: 'c1', provider_name: 'QA Mock Upstream', suffix: 'abcd', credential_type: 'api_key', priority: 100, enabled: true }]
    vi.stubGlobal('fetch', mockApi([...channels, ...credentials]))
    renderPage(<ChannelsPage />)
    expect((await screen.findAllByText('OpenAI 兼容')).length).toBeGreaterThan(0)
    expect(screen.queryByText('openai_compatible')).not.toBeInTheDocument()
    await userEvent.click(screen.getByRole('tab', { name: i18n.t('credentials', { lng: 'zh-CN' }) }))
    expect((await screen.findAllByText(i18n.t('credentialTypeApiKey', { lng: 'zh-CN' }))).length).toBeGreaterThan(0)
    expect(screen.queryByText('api_key')).not.toBeInTheDocument()
  })

  it('translates the API-key type instead of printing the enum', async () => {
    const keys = [{ id: 'a1', name: 'QA traffic key', key_prefix: 'pk_live_12', budget_micros: null, enabled: true, key_type: 'service', profile_id: null, expires_at: null, allowed_ips_json: '[]', denied_ips_json: '[]' }]
    vi.stubGlobal('fetch', mockApi([], keys))
    renderPage(<AccessPage />)
    expect((await screen.findAllByText(i18n.t('keyTypeService', { lng: 'zh-CN' }))).length).toBeGreaterThan(0)
    expect(screen.queryByText('service')).not.toBeInTheDocument()
  })

  it('translates capability names and leaves an unknown one verbatim', () => {
    render(<CapabilityTags value={['chat', 'responses', 'embeddings', 'provider_specific_thing']} max={4} />)
    expect(screen.getByText(i18n.t('capabilityChat', { lng: 'zh-CN' }))).toBeInTheDocument()
    expect(screen.getByText(i18n.t('capabilityResponses', { lng: 'zh-CN' }))).toBeInTheDocument()
    expect(screen.getByText(i18n.t('capabilityEmbeddings', { lng: 'zh-CN' }))).toBeInTheDocument()
    expect(screen.getByText('provider_specific_thing')).toBeInTheDocument()
  })

  it('resolves every new key in both languages, and names the notification region', () => {
    for (const key of ['togglePasswordVisibility', 'notifications', 'streamResponse', 'streamResponseHint', 'playgroundNoOutput', 'credentialTypeApiKey', 'keyTypeService', 'keyTypeUser', 'keyTypePersonal', 'capabilityChat', 'capabilityResponses', 'capabilityEmbeddings']) {
      for (const language of ['en', 'zh-CN']) expect([key, language, i18n.exists(key, { lng: language })]).toEqual([key, language, true])
    }
    expect(i18n.t('togglePasswordVisibility', { lng: 'en' })).toBe('Toggle password visibility')
    expect(i18n.t('notifications', { lng: 'en' })).toBe('Notifications')
  })
})
