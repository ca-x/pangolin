import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import ChannelsPage from './ChannelsPage'

const json = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }))
const project = { id: 'p1', name: 'Project', slug: 'project', owner_user_id: 'u1', is_default: true, enabled: true }
/** A saved channel whose base URL was edited by hand: the value an edit must never overwrite. */
const savedChannel = { id: 'c1', name: 'Primary', kind: 'openai', base_url: 'https://custom.example.com/v1', enabled: true }

function mockApi() {
  const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
    const path = String(input)
    if (path.endsWith('/projects')) return json([project])
    if (path.includes('/permissions')) return json(['*'])
    if (init?.method === 'POST') return json({ id: 'c2' })
    return json({ data: [savedChannel], total: 1, offset: 0, limit: 25 })
  })
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

function renderPage() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><ChannelsPage /></ProjectProvider></MemoryRouter></QueryClientProvider>)
}

/** The picker is a labelled radio list, so the provider group is found by the field label. */
const providerGroup = () => screen.findByRole('radiogroup', { name: 'Type' })
// A required field renders its asterisk inside the label, so the query is a prefix match.
const baseUrlInput = () => screen.getByLabelText(/^Base URL/)

// The 18 adapter kinds the channel form can create, each with a bundled catalog
// icon. Mirrored here on purpose: the picker's observable contract is that a
// provider is a single-select entry with an icon, not free text.
const PROVIDER_LABEL_KEYS = [
  'providerLabelOpenai', 'providerLabelOpenaiCompatible', 'providerLabelAnthropic', 'providerLabelGemini',
  'providerLabelAzure', 'providerLabelBedrock', 'providerLabelVertex', 'providerLabelGcp',
  'providerLabelOpenrouter', 'providerLabelDeepseek', 'providerLabelMoonshot', 'providerLabelZhipu',
  'providerLabelDoubao', 'providerLabelXai', 'providerLabelGroq', 'providerLabelOllama',
  'providerLabelNanogpt', 'providerLabelJina',
]

describe('channel provider picker', () => {
  beforeEach(() => { void i18n.changeLanguage('en') })

  it('offers every provider as an icon list and keeps the type field in sync', async () => {
    mockApi()
    renderPage()
    await userEvent.click(await screen.findByRole('button', { name: 'Add channel' }))
    const group = await providerGroup()
    const radios = within(group).getAllByRole('radio')
    expect(radios).toHaveLength(PROVIDER_LABEL_KEYS.length)
    // Icons come from the bundled catalog icon map, not from copied brand assets.
    for (const name of ['OpenAI', 'Anthropic', 'Google Gemini', 'Ollama', 'Doubao', 'OpenRouter', 'Jina AI']) {
      expect(within(group).getByRole('radio', { name }).querySelector('svg')).not.toBeNull()
    }
    // The hidden field carries the type, so the submitted payload shape is unchanged.
    expect(document.querySelector('input[name="kind"]')).toHaveValue('openai')
    await userEvent.click(within(group).getByRole('radio', { name: 'Anthropic' }))
    expect(document.querySelector('input[name="kind"]')).toHaveValue('anthropic')
    expect(within(group).getByRole('radio', { name: 'Anthropic' })).toHaveAttribute('aria-checked', 'true')
  })

  it('writes the picked provider default into the base URL while creating', async () => {
    mockApi()
    renderPage()
    await userEvent.click(await screen.findByRole('button', { name: 'Add channel' }))
    const group = await providerGroup()
    expect(baseUrlInput()).toHaveValue('https://api.openai.com/v1')
    await userEvent.click(within(group).getByRole('radio', { name: 'Ollama' }))
    expect(baseUrlInput()).toHaveValue('http://127.0.0.1:11434/v1')
    await userEvent.click(within(group).getByRole('radio', { name: 'Google Gemini' }))
    expect(baseUrlInput()).toHaveValue('https://generativelanguage.googleapis.com')
  })

  it('never overwrites a saved base URL when the provider changes while editing', async () => {
    const fetchMock = mockApi()
    renderPage()
    await userEvent.click((await screen.findAllByRole('button', { name: 'Edit' }))[0])
    const group = await providerGroup()
    expect(baseUrlInput()).toHaveValue(savedChannel.base_url)
    await userEvent.click(within(group).getByRole('radio', { name: 'Anthropic' }))
    expect(baseUrlInput()).toHaveValue(savedChannel.base_url)
    await userEvent.click(screen.getByRole('button', { name: 'Save' }))
    await vi.waitFor(() => {
      const call = fetchMock.mock.calls.find(([, init]) => init?.method === 'POST')
      expect(call).toBeTruthy()
      expect(JSON.parse(String(call?.[1]?.body))).toMatchObject({ id: 'c1', kind: 'anthropic', base_url: savedChannel.base_url })
    })
  })

  it('shows the target of the dialog that is open, not the previous one', async () => {
    mockApi()
    renderPage()
    await userEvent.click(await screen.findByRole('button', { name: 'Add channel' }))
    await userEvent.click(within(await providerGroup()).getByRole('radio', { name: 'Anthropic' }))
    await userEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    await userEvent.click((await screen.findAllByRole('button', { name: 'Edit' }))[0])
    // The edit dialog belongs to the saved openai row: a selection left over
    // from the create dialog would submit the wrong provider.
    expect(within(await providerGroup()).getByRole('radio', { name: 'OpenAI' })).toHaveAttribute('aria-checked', 'true')
    expect(document.querySelector('input[name="kind"]')).toHaveValue('openai')
  })

  it('names every provider in both languages', () => {
    for (const key of PROVIDER_LABEL_KEYS) {
      for (const language of ['en', 'zh-CN']) {
        expect([key, language, i18n.exists(key, { lng: language })]).toEqual([key, language, true])
      }
    }
  })

  it('localizes the picker', async () => {
    mockApi()
    renderPage()
    await userEvent.click(await screen.findByRole('button', { name: 'Add channel' }))
    await providerGroup()
    await i18n.changeLanguage('zh-CN')
    expect(await screen.findByRole('radio', { name: '智谱 AI' })).toBeInTheDocument()
    await i18n.changeLanguage('en')
  })
})
