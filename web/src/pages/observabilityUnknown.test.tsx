import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../i18n'
import { ObservabilityNotice, useObservability } from '../observability'

/**
 * `/settings/request-logging` is instance-owner-only, so a project member's read
 * of the logging policy fails. The console used to treat that failure as
 * "logging is on" and print a measured zero — the one place it invented a fact
 * it did not have. An unreadable policy is unknown: every figure stays `—` and
 * the notice says so.
 */
const json = (value: unknown) => Promise.resolve(new Response(JSON.stringify(value), { status: 200, headers: { 'Content-Type': 'application/json' } }))
const bootstrap = { initialized: true, authenticated: true, user: null, capture_payloads: false, observability_available: true, branding: { instance_name: 'Pangolin', branding_name: 'Pangolin', favicon_url: '/logo.webp', onboarding_complete: true } }

type Options = { policy?: unknown; policyFails?: boolean; available?: boolean }
const mockApi = (options: Options = {}) => vi.fn((input: RequestInfo | URL) => {
  const path = String(input)
  if (path.includes('/api/v1/bootstrap')) return json({ ...bootstrap, observability_available: options.available ?? true })
  if (path.includes('settings/request-logging')) {
    if (options.policyFails) return Promise.resolve(new Response(JSON.stringify({ error: { message: 'forbidden' } }), { status: 403, headers: { 'Content-Type': 'application/json' } }))
    return json(options.policy ?? { enabled: true, default_level: 'metadata', key_override_enabled: false, key_disable_allowed: false })
  }
  return json({})
})

function Probe() {
  const observability = useObservability()
  return <>
    <span data-testid="reason">{observability.reason}</span>
    <span data-testid="measured">{String(observability.measured)}</span>
    <ObservabilityNotice observability={observability} />
  </>
}
const renderProbe = () => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(<QueryClientProvider client={client}><MemoryRouter><Probe /></MemoryRouter></QueryClientProvider>)
}

describe('an unreadable request-logging policy is unknown, not "recording on"', () => {
  beforeEach(() => { void i18n.changeLanguage('en') })

  it('reports unknown when the policy read is refused', async () => {
    vi.stubGlobal('fetch', mockApi({ policyFails: true }))
    renderProbe()
    await vi.waitFor(() => expect(screen.getByTestId('reason')).toHaveTextContent('unknown'))
    expect(screen.getByTestId('measured')).toHaveTextContent('false')
    expect(await screen.findByRole('alert')).toHaveTextContent('Recording state unknown')
    // The old behaviour claimed the opposite of what it knew.
    expect(screen.queryByText(/not being recorded/i)).not.toBeInTheDocument()
  })

  it('reports unknown when the stored policy is not a policy at all', async () => {
    vi.stubGlobal('fetch', mockApi({ policy: 'not-a-policy' }))
    renderProbe()
    await vi.waitFor(() => expect(screen.getByTestId('reason')).toHaveTextContent('unknown'))
    expect(screen.getByTestId('measured')).toHaveTextContent('false')
  })

  it('still reports a readable policy that records events as measured', async () => {
    vi.stubGlobal('fetch', mockApi())
    renderProbe()
    await vi.waitFor(() => expect(screen.getByTestId('reason')).toHaveTextContent('measured'))
    expect(screen.getByTestId('measured')).toHaveTextContent('true')
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
  })

  it('keeps a readable policy that is off distinct from an unreadable one', async () => {
    vi.stubGlobal('fetch', mockApi({ policy: { enabled: true, default_level: 'off', key_override_enabled: false, key_disable_allowed: false } }))
    renderProbe()
    await vi.waitFor(() => expect(screen.getByTestId('reason')).toHaveTextContent('not_recorded'))
    expect(await screen.findByRole('alert')).toHaveTextContent('Requests are not being recorded')
  })
})
