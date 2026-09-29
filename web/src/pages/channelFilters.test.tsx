import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import ChannelsPage from './ChannelsPage'

const json = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }))
const channels = [
  { id: 'c1', name: 'Primary', kind: 'openai', base_url: 'https://example.test/v1', priority: 100, enabled: true },
  { id: 'c2', name: 'Backup', kind: 'anthropic', base_url: 'https://example.test', priority: 100, enabled: false },
]

describe('channel facets', () => {
  beforeEach(async () => { await i18n.changeLanguage('en') })

  it('filters by provider and state before paging, and clears an empty result', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([{ id: 'p1', name: 'Project', slug: 'p', enabled: true, is_default: true }])
      if (path.includes('/permissions')) return json(['*'])
      if (path.includes('/operations/channels?')) {
        const params = new URL(path, 'http://localhost').searchParams
        const data = channels.filter((row) => (!params.has('kind') || params.get('kind') === row.kind) && (!params.has('enabled') || params.get('enabled') === String(row.enabled)))
        return json({ data, total: data.length })
      }
      return json({ data: [], total: 0 })
    })
    vi.stubGlobal('fetch', fetchMock)
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><ChannelsPage /></ProjectProvider></MemoryRouter></QueryClientProvider>)

    const table = await screen.findByRole('region', { name: 'Channels' })
    await userEvent.click(within(table).getByRole('button', { name: /View base URL: https:\/\/example\.test\/v1/ }))
    const urlDialog = screen.getByRole('dialog', { name: 'Base URL · Primary' })
    expect(within(urlDialog).getByText('https://example.test/v1')).toBeInTheDocument()
    await userEvent.click(within(urlDialog).getAllByRole('button', { name: 'Close' })[0])
    expect(within(document.querySelector('.mobile-resource-list') as HTMLElement).getByText('https://example.test/v1')).toBeInTheDocument()

    await userEvent.click(screen.getByRole('combobox', { name: 'Provider type' }))
    await userEvent.click(await screen.findByRole('option', { name: 'Anthropic' }))
    await userEvent.click(screen.getByRole('combobox', { name: 'Status' }))
    await userEvent.click(await screen.findByRole('option', { name: 'Disabled' }))
    expect((await screen.findAllByText('Backup')).length).toBeGreaterThan(0)
    await waitFor(() => expect(fetchMock.mock.calls.some(([input]) => String(input).includes('kind=anthropic') && String(input).includes('enabled=false'))).toBe(true))

    await userEvent.click(screen.getByRole('combobox', { name: 'Provider type' }))
    await userEvent.click(await screen.findByRole('option', { name: 'DeepSeek' }))
    await userEvent.click(await screen.findByRole('button', { name: 'Clear filters' }))
    expect((await screen.findAllByText('Primary')).length).toBeGreaterThan(0)
    expect(screen.getByRole('combobox', { name: 'Provider type' })).toHaveValue('All provider types')
  })
})
