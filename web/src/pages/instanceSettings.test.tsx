import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { BrandingSettings } from './SystemPage'

const json = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }))
const settings = {
  instance_name: 'Pangolin', branding_name: 'Pangolin / 鲮鲤', favicon_url: '/logo.webp', onboarding_complete: true,
  currency: 'USD', timezone: 'UTC', retry_policy: { version: 1, attempts: 1, error_mode: 'normalized' },
  quota_collection_enabled: true, quota_routing_mode: 'REMOVE_ON_EXHAUSTED',
}

describe('instance scheduling and routing settings', () => {
  beforeEach(async () => { await i18n.changeLanguage('en'); vi.clearAllMocks() })

  it('round-trips currency, timezone, retry defaults, collection, and routing mode', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.includes('/settings/system')) return init?.method === 'PUT' ? json({ ok: true }) : json(settings)
      return json({ data: [], total: 0 })
    })
    vi.stubGlobal('fetch', fetchMock)
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><BrandingSettings /></QueryClientProvider>)

    const currency = await screen.findByRole('textbox', { name: /currency/i })
    await userEvent.clear(currency)
    await userEvent.type(currency, 'EUR')
    const timezone = screen.getByRole('textbox', { name: /timezone/i })
    await userEvent.clear(timezone)
    await userEvent.type(timezone, 'Europe/Berlin')
    const retry = screen.getByRole('textbox', { name: /retry/i })
    fireEvent.change(retry, { target: { value: '{"version":1,"attempts":3,"error_mode":"pass_through"}' } })
    await userEvent.click(screen.getByRole('combobox', { name: /quota.*routing/i }))
    await userEvent.click(await screen.findByRole('option', { name: /ignore.*quota/i }))
    await userEvent.click(screen.getByRole('switch', { name: /collect.*quota/i }))
    await userEvent.click(screen.getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(fetchMock.mock.calls.some(([, init]) => init?.method === 'PUT')).toBe(true))
    const [, init] = fetchMock.mock.calls.find(([, request]) => request?.method === 'PUT') as [string, RequestInit]
    expect(JSON.parse(String(init.body))).toMatchObject({
      currency: 'EUR', timezone: 'Europe/Berlin',
      retry_policy: { version: 1, attempts: 3, error_mode: 'pass_through' },
      quota_collection_enabled: false, quota_routing_mode: 'IGNORE_QUOTA',
    })
  }, 15_000)
})
