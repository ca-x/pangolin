import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes, useLocation, useNavigate } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import ChannelsPage from './ChannelsPage'
import AccessPage from './AccessPage'
import SystemPage from './SystemPage'
import { ThemeProvider } from '../theme'

const json = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }))

function BrowserControls() {
  const location = useLocation()
  const navigate = useNavigate()
  return <><output data-testid="address">{location.pathname + location.search}</output><button onClick={() => navigate(-1)}>Back</button><ChannelsPage /></>
}

function Address() {
  const location = useLocation()
  return <output data-testid="address">{location.pathname + location.search}</output>
}

describe('management view addresses', () => {
  beforeEach(async () => { await i18n.changeLanguage('en') })

  it('opens a direct channel subview and restores it with browser Back', async () => {
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => String(input).endsWith('/projects')
      ? json([{ id: 'p1', name: 'Project A', slug: 'a', enabled: true, is_default: true }])
      : String(input).includes('/permissions') ? json(['*']) : json({ data: [], total: 0 })))
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><MemoryRouter initialEntries={['/channels?tab=credentials']}><ProjectProvider><Routes><Route path="/channels" element={<BrowserControls />} /></Routes></ProjectProvider></MemoryRouter></QueryClientProvider>)

    expect(await screen.findByRole('tab', { name: 'Credentials' })).toHaveAttribute('aria-selected', 'true')
    await userEvent.click(screen.getByRole('tab', { name: 'Channels' }))
    expect(screen.getByTestId('address')).toHaveTextContent('/channels')
    expect(screen.getByRole('tab', { name: 'Channels' })).toHaveAttribute('aria-selected', 'true')
    await userEvent.click(screen.getByRole('button', { name: 'Back' }))
    expect(await screen.findByRole('tab', { name: 'Credentials' })).toHaveAttribute('aria-selected', 'true')
    expect(screen.getByTestId('address')).toHaveTextContent('/channels?tab=credentials')
  })

  it('replaces a forbidden access tab in the address with the permitted view', async () => {
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => String(input).endsWith('/projects')
      ? json([{ id: 'p1', name: 'Project A', slug: 'a', enabled: true, is_default: true }])
      : String(input).includes('/permissions') ? json(['project:read'])
        : String(input).endsWith('/roles') ? json([]) : json({ data: [], total: 0 })))
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><MemoryRouter initialEntries={['/access?tab=users']}><ProjectProvider><Routes><Route path="/access" element={<><Address /><AccessPage /></>} /></Routes></ProjectProvider></MemoryRouter></QueryClientProvider>)

    expect(await screen.findByRole('tab', { name: 'Members' })).toHaveAttribute('aria-selected', 'true')
    await waitFor(() => expect(screen.getByTestId('address')).toHaveTextContent('/access?tab=members'))
  })

  it('normalizes an unavailable system tab after permissions resolve', async () => {
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => String(input).endsWith('/projects')
      ? json([{ id: 'p1', name: 'Project A', slug: 'a', enabled: true, is_default: true }])
      : String(input).includes('/permissions') ? json([])
        : Promise.resolve(new Response('{}', { status: 503, headers: { 'Content-Type': 'application/json' } }))))
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><ThemeProvider><MemoryRouter initialEntries={['/system?tab=storage']}><ProjectProvider><Routes><Route path="/system" element={<><Address /><SystemPage /></>} /></Routes></ProjectProvider></MemoryRouter></ThemeProvider></QueryClientProvider>)

    expect(await screen.findByRole('tab', { name: 'Appearance' })).toHaveAttribute('aria-selected', 'true')
    await waitFor(() => expect(screen.getByTestId('address')).toHaveTextContent('/system'))
  })
})
