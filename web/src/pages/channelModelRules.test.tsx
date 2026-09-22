import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import ChannelsPage from './ChannelsPage'

const json = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }))
const project = { id: 'p1', name: 'Project', slug: 'project', owner_user_id: 'u1', is_default: true, enabled: true }
const settings = {
  id: 'c1',
  provider_id: 'c1',
  endpoint_mappings: { version: 1 },
  model_rules: { version: 1, lowercase: true, mappings: { legacy: 'upstream-legacy' } },
  parameter_overrides: { version: 1 },
  retry_statuses: { version: 1, statuses: [429] },
  auto_disable_policy: { version: 1, enabled: false },
}

function renderPage() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><ChannelsPage /></ProjectProvider></MemoryRouter></QueryClientProvider>)
}

describe('channel model rules editor', () => {
  beforeEach(() => { void i18n.changeLanguage('en') })

  it('submits structured transformations as the model-rules document', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      if (path.includes('operations/channels')) return json({ data: [{ id: 'c1', name: 'Primary' }], total: 1 })
      if (path.includes('operations/channel-settings') && init?.method === 'POST') return json({ id: 'c1' })
      if (path.includes('operations/channel-settings')) return json({ data: [settings], total: 1, offset: 0, limit: 25 })
      return json({ data: [], total: 0 })
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage()

    await userEvent.click(await screen.findByRole('tab', { name: 'Channel policies' }))
    await userEvent.click((await screen.findAllByRole('button', { name: 'Edit' }))[0])
    const prefixes = await screen.findByRole('textbox', { name: 'Auto-trim prefixes' })
    await userEvent.type(prefixes, 'vendor, openai')
    await userEvent.click(screen.getByRole('switch', { name: /Hide original names/ }))
    await userEvent.click(screen.getByRole('switch', { name: /Hide transformed names/ }))
    await userEvent.click(screen.getByRole('button', { name: 'Add mapping' }))
    await userEvent.type(screen.getByRole('textbox', { name: 'Original name 2' }), 'alias')
    await userEvent.type(screen.getByRole('textbox', { name: 'Transformed name 2' }), 'provider/model')
    await userEvent.click(screen.getByRole('button', { name: 'Save' }))

    await vi.waitFor(() => {
      const call = fetchMock.mock.calls.find(([path, init]) => String(path).includes('operations/channel-settings') && init?.method === 'POST')
      expect(call).toBeTruthy()
      const body = JSON.parse(String(call?.[1]?.body))
      expect(body.model_rules).toEqual({
        version: 1,
        lowercase: true,
        auto_trim_prefixes: ['vendor', 'openai'],
        hide_original: true,
        hide_mapped: true,
        mappings: { legacy: 'upstream-legacy', alias: 'provider/model' },
      })
    })
  }, 15_000)

  it('localizes every structured editor label', () => {
    for (const key of ['modelRulesHint', 'autoTrimPrefixes', 'hideOriginalModels', 'hideMappedModels', 'addMapping', 'mappingSource', 'mappingTarget', 'advancedModelRules']) {
      expect(i18n.exists(key, { lng: 'en' })).toBe(true)
      expect(i18n.exists(key, { lng: 'zh-CN' })).toBe(true)
    }
  })

  it.each([
    ['array', '[]'],
    ['null', 'null'],
    ['string', '"text"'],
  ])('rejects a parsed %s in advanced JSON', async (_kind, document) => {
    const fetchMock = vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      if (path.includes('operations/channels')) return json({ data: [{ id: 'c1', name: 'Primary' }], total: 1 })
      if (path.includes('operations/channel-settings')) return json({ data: [settings], total: 1, offset: 0, limit: 25 })
      return json({ data: [], total: 0 })
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage()

    await userEvent.click(await screen.findByRole('tab', { name: 'Channel policies' }))
    await userEvent.click((await screen.findAllByRole('button', { name: 'Edit' }))[0])
    await userEvent.click(await screen.findByRole('button', { name: 'Advanced model rules JSON' }))
    const advanced = await screen.findByRole('textbox', { name: 'Advanced model rules JSON' })
    fireEvent.change(advanced, { target: { value: document } })

    expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled()
  })
})
