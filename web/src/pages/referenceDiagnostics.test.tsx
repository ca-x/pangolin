import { screen, within } from '@testing-library/react'
import { beforeEach, expect, it } from 'vitest'
import { Route, Routes } from 'react-router'
import i18n from '../i18n'
import { TraceDetailPage } from './OperationsPage'
import { json, mockApi, mount } from './referenceConsoleFixtures'
beforeEach(async () => {
  localStorage.clear()
  await i18n.changeLanguage('en')
})
it.each(['en', 'zh-CN'])(
  'shows safe localized diagnostics on separate attempts without captured payload in %s',
  async (language) => {
    await i18n.changeLanguage(language)
    const attempts = [
      {
        id: 'e1',
        request_id: 'r1',
        attempt: 1,
        status: 'failed',
        provider_id: 'c1',
        conversion_diagnostics: [
          { phase: 'request', code: 'roles_normalized', reason: 'roles_normalized', path: 'messages[].role' },
        ],
        affinity_diagnostics: [
          {
            rule_id: 'rule-one',
            scope_digest: 'a'.repeat(64),
            reason: 'hit',
            provider_id: 'c1',
            expires_at: 1800000000,
          },
        ],
      },
      {
        id: 'e2',
        request_id: 'r1',
        attempt: 2,
        status: 'succeeded',
        provider_id: 'c2',
        conversion_diagnostics: [
          {
            phase: 'response',
            code: 'unsupported_response_shape',
            reason: 'unsupported_response_shape',
            path: 'response',
          },
        ],
        affinity_diagnostics: [
          { rule_id: 'rule-two', scope_digest: 'b'.repeat(64), reason: 'established', provider_id: 'c2' },
        ],
      },
    ]
    mount(
      <Routes>
        <Route path="/operations/traces/:id" element={<TraceDetailPage />} />
      </Routes>,
      '/operations/traces/t1',
      mockApi((path) =>
        path.includes('/trace-detail')
          ? json({
              trace: { id: 't1', started_at: 1, finished_at: 2, status: 'succeeded' },
              requests: [],
              executions: attempts,
              usage: [],
              cost_items: [],
            })
          : undefined,
      ),
    )
    const first = within(await screen.findByRole('region', { name: `${i18n.t('attempt')} #1` }))
    const second = within(screen.getByRole('region', { name: `${i18n.t('attempt')} #2` }))
    expect(first.getByText(i18n.t('conversion_roles_normalized'))).toBeInTheDocument()
    expect(first.getByText('messages[].role')).toBeInTheDocument()
    expect(first.queryByText('rule-two')).not.toBeInTheDocument()
    expect(second.getByText(i18n.t('conversion_unsupported_response_shape'))).toBeInTheDocument()
    expect(second.queryByText('rule-one')).not.toBeInTheDocument()
    expect(screen.queryByText('"phase"')).not.toBeInTheDocument()
  },
)
it('handles malformed or arbitrary diagnostic values without rendering their contents', async () => {
  const { ExecutionDiagnostics } = await import('./ExecutionDiagnostics')
  mount(
    <ExecutionDiagnostics
      attempt={{
        attempt: 1,
        conversion_diagnostics: [
          {
            phase: 'response',
            code: 'unsupported_response_shape',
            reason: 'secret diagnostic',
            path: '/arbitrary-secret-path',
          },
        ],
        affinity_diagnostics: [
          { rule_id: 'rule-one', scope_digest: 'a'.repeat(64), reason: 'hit', provider_id: '<secret-provider>' },
        ],
      }}
    />,
    '/operations',
    mockApi(),
  )
  expect(await screen.findByText(i18n.t('noDiagnostics'))).toBeInTheDocument()
  expect(screen.queryByText(/secret/)).not.toBeInTheDocument()
})
it('shows loading while the exact request record is pending instead of inventing an empty diagnostic result', async () => {
  const { RequestDetailPage } = await import('./OperationsPage')
  const { waitFor } = await import('@testing-library/react')
  let finish: ((value: Response) => void) | undefined
  const detail = {
    id: 'r1',
    request_id: 'public-r1',
    trace_id: null,
    input_tokens: 0,
    output_tokens: 0,
    cost_micros: 0,
    latency_ms: 1,
    started_at: 1,
    status_code: 200,
    usage_measurement: { version: 1, input_tokens: true, output_tokens: true },
    pricing_status: 'priced',
  }
  const fetchMock = mockApi((path) =>
    path.includes('/observability/requests/r1')
      ? json(detail)
      : path.endsWith('/operations/requests/r1')
        ? new Promise<Response>((resolve) => {
            finish = resolve
          })
        : undefined,
  )
  mount(
    <Routes>
      <Route path="/operations/requests/:id" element={<RequestDetailPage />} />
    </Routes>,
    '/operations/requests/r1',
    fetchMock,
  )
  await waitFor(() => expect(finish).toBeDefined())
  expect(screen.getByRole('status')).toHaveAccessibleName(i18n.t('loading'))
  expect(screen.queryByRole('region', { name: i18n.t('attemptDiagnostics') })).not.toBeInTheDocument()
  finish!(await json({ executions: [], usage: [], cost_items: [] }))
  expect(await screen.findByRole('region', { name: i18n.t('attemptDiagnostics') })).toBeInTheDocument()
})
it('keeps valid diagnostics readable when a safe integer expiry is outside the Date range', async () => {
  const { ExecutionDiagnostics } = await import('./ExecutionDiagnostics')
  const base = { scope_digest: 'a'.repeat(64), reason: 'hit', provider_id: 'c1' }
  mount(
    <ExecutionDiagnostics
      attempt={{
        attempt: 1,
        conversion_diagnostics: [
          { phase: 'request', code: 'roles_normalized', reason: 'roles_normalized', path: 'messages[].role' },
        ],
        affinity_diagnostics: [
          { ...base, rule_id: 'out-of-range', expires_at: 100000000000000 },
          { ...base, rule_id: 'valid-date', expires_at: 1800000000 },
          { ...base, rule_id: 'no-date', expires_at: null },
        ],
      }}
    />,
    '/operations',
    mockApi(),
  )
  expect(await screen.findByText('out-of-range')).toBeInTheDocument()
  expect(screen.getByText('valid-date')).toBeInTheDocument()
  expect(screen.getByText('no-date')).toBeInTheDocument()
  expect(screen.getByText(i18n.t('conversion_roles_normalized'))).toBeInTheDocument()
  const invalid = screen.getByText('out-of-range').closest('.mantine-Stack-root')!
  expect(invalid).toHaveTextContent(`${i18n.t('affinityExpiry')}: —`)
  const valid = screen.getByText('valid-date').closest('.mantine-Stack-root')!
  expect(valid).not.toHaveTextContent(`${i18n.t('affinityExpiry')}: —`)
})
