import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, within } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import AccessPage from './AccessPage'

/**
 * `src/access.rs::ScopedApiKeyView` now carries `last_used_at` beside
 * `expires_at` and `spent_micros`, so a key that nobody has authenticated with
 * for months is distinguishable from one in active use. The table showed the
 * state and the expiry only, and a key that was never used must read `—`, not
 * the epoch or "0".
 */
const json = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }))
const project = { id: 'p1', name: 'Project A', slug: 'project-a', owner_user_id: 'u1', is_default: true, enabled: true }
const key = (overrides: Record<string, unknown>) => ({ id: 'k', name: 'key', key_prefix: 'pk', key_type: 'service', profile_id: null, allowed_ips_json: '[]', denied_ips_json: '[]', enabled: true, expires_at: null, budget_micros: null, spent_micros: 0, last_used_at: null, ...overrides })

const renderPage = () => {
  vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
    const path = String(input)
    if (path.endsWith('/projects')) return json([project])
    if (path.includes('/permissions')) return json(['api_key:manage'])
    if (path.includes('/api-keys')) return json([
      key({ id: 'k1', name: 'active-key', last_used_at: 1_800_000_000 }),
      key({ id: 'k2', name: 'never-used-key' }),
    ])
    return json([])
  }))
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><AccessPage /></ProjectProvider></MemoryRouter></QueryClientProvider>)
}

describe('the key table reports when a key was last used', () => {
  beforeEach(async () => { await i18n.changeLanguage('en') })

  it('shows the last use beside the expiry and the state', async () => {
    renderPage()
    expect(await screen.findByRole('columnheader', { name: 'Last used' })).toBeInTheDocument()

    const used = await screen.findByRole('row', { name: /active-key/ })
    const expected = new Intl.DateTimeFormat('en', { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(1_800_000_000_000))
    // The column is found by its header, not by position: adding the selection
    // column shifted every index and this assertion silently read the state cell.
    const column = (header: string) => screen.getAllByRole('columnheader').findIndex((cell) => cell.textContent === header)
    expect(within(used).getAllByRole('cell')[column('Last used')]).toHaveTextContent(expected)

    // Unused is unmeasured: the last-used cell must not read as a time or as zero.
    const unused = screen.getByRole('row', { name: /never-used-key/ })
    expect(within(unused).getAllByRole('cell')[column('Last used')]).toHaveTextContent('—')
    expect(within(unused).queryByText('1800000000')).not.toBeInTheDocument()
  })
})
