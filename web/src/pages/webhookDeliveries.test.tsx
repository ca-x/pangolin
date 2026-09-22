import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import { ThemeProvider } from '../theme'
import SystemPage from './SystemPage'

/**
 * `webhook_deliveries` is the only record of whether an event reached a
 * webhook, and the admin API projects it as the `webhook-deliveries` resource
 * (`src/api/operations_api.rs`) — but nothing under `web/src` read it, so a
 * failing webhook was visible only as a `webhook` job row with no attempt, no
 * response and no next attempt. The projection carries no created/finished
 * timestamp and no error text, so those read `—` rather than a guess.
 */
const json = (value: unknown, status = 200) => Promise.resolve(new Response(JSON.stringify(value), { status, headers: { 'Content-Type': 'application/json' } }))
const paged = (rows: unknown[], total = rows.length) => json({ data: rows, total, offset: 0, limit: 25 })
const project = { id: 'p1', name: 'Project A', slug: 'project-a', owner_user_id: 'u1', is_default: true, enabled: true }
const hook = { id: 'w1', name: 'Ops hook', url: 'https://hooks.example.test/ops', subscriptions: ['request.failed'], enabled: true }
const formatted = (seconds: number) => new Intl.DateTimeFormat('en', { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(seconds * 1000))

const renderPage = () => {
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
}

describe('webhook delivery history', () => {
  beforeEach(async () => { await i18n.changeLanguage('en') })

  it('shows each attempt with its webhook, status, response and next attempt', async () => {
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      if (path.includes('/operations/webhooks')) return paged([hook])
      if (path.includes('/operations/webhook-deliveries')) return paged([
        { id: 'd1', webhook_id: 'w1', event_type: 'request.failed', attempt: 3, status: 'failed', response_status: 503, next_attempt_at: 1_800_000_000 },
        { id: 'd2', webhook_id: 'w1', event_type: 'channel.disabled', attempt: 1, status: 'succeeded', response_status: null, next_attempt_at: null },
      ])
      return paged([])
    }))
    renderPage()
    await userEvent.click(await screen.findByRole('tab', { name: 'Webhooks' }))

    expect(await screen.findByRole('heading', { name: 'Delivery attempts' })).toBeInTheDocument()
    // The webhooks table above lists the same hook, so the attempts are read
    // from their own labelled region.
    const panel = screen.getByRole('region', { name: 'Delivery attempts' })
    expect(within(panel).getByRole('columnheader', { name: 'Webhook' })).toBeInTheDocument()
    expect(within(panel).getByRole('columnheader', { name: 'Next attempt' })).toBeInTheDocument()

    const failed = within(panel).getByRole('row', { name: /request\.failed/ })
    expect(within(failed).getByText('request.failed')).toBeInTheDocument()
    expect(within(failed).getByText('Failed')).toBeInTheDocument()
    expect(within(failed).getByText('503')).toBeInTheDocument()
    expect(within(failed).getByText(formatted(1_800_000_000))).toBeInTheDocument()

    // A delivered attempt has no next attempt: the projection stores NULL, which
    // is unrecorded, not "0" and not a timestamp.
    const succeeded = within(panel).getByRole('row', { name: /channel.disabled/ })
    expect(within(succeeded).getByText('Succeeded')).toBeInTheDocument()
    expect(within(succeeded).getAllByText('—')).toHaveLength(2)
  })

  it('reports the empty history instead of an empty table', async () => {
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      if (path.includes('/operations/webhooks')) return paged([hook])
      return paged([])
    }))
    renderPage()
    await userEvent.click(await screen.findByRole('tab', { name: 'Webhooks' }))

    expect(await screen.findByText('No webhook delivery has been attempted yet.')).toBeInTheDocument()
    expect(screen.queryByRole('region', { name: 'Delivery attempts' })).not.toBeInTheDocument()
  })

  it('reports a failed read and retries it', async () => {
    let attempts = 0
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.endsWith('/projects')) return json([project])
      if (path.includes('/permissions')) return json(['*'])
      if (path.includes('/operations/webhooks')) return paged([hook])
      if (path.includes('/operations/webhook-deliveries')) {
        attempts += 1
        if (attempts === 1) return json({ error: { message: 'projection unavailable' } }, 503)
        return paged([{ id: 'd1', webhook_id: 'w1', event_type: 'request.failed', attempt: 1, status: 'pending', response_status: null, next_attempt_at: 1_800_000_000 }])
      }
      return paged([])
    }))
    renderPage()
    await userEvent.click(await screen.findByRole('tab', { name: 'Webhooks' }))

    const banner = await screen.findByText('The request failed. Try again shortly.')
    await userEvent.click(within(banner.closest('.query-error') as HTMLElement).getByRole('button', { name: 'Retry' }))
    expect(await screen.findByText('Pending')).toBeInTheDocument()
  })
})
