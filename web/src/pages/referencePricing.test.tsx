import { screen, within } from '@testing-library/react'
import { beforeEach, expect, it } from 'vitest'
import i18n from '../i18n'
import OperationsPage from './OperationsPage'
import { json, mockApi, mount } from './referenceConsoleFixtures'
beforeEach(async () => {
  localStorage.clear()
  await i18n.changeLanguage('en')
})
it('keeps price, counter measurement and conservative settlement independent in operational usage rows', async () => {
  const rows = [
    {
      id: 'unknown',
      execution_id: 'unknown',
      input_tokens: 0,
      output_tokens: 0,
      cost_micros: 0,
      pricing_status: 'missing_price',
      settlement_kind: 'reported',
      usage_measurement: { version: 1, input_tokens: false, output_tokens: true, cache_read_tokens: false },
    },
    {
      id: 'free',
      execution_id: 'free',
      input_tokens: 0,
      output_tokens: 0,
      cost_micros: 0,
      pricing_status: 'explicit_free',
      settlement_kind: 'reported',
      usage_measurement: { version: 1, input_tokens: true, output_tokens: true, cache_read_tokens: false },
    },
    {
      id: 'legacy',
      execution_id: 'legacy',
      input_tokens: 7,
      output_tokens: 8,
      cost_micros: 123,
      pricing_status: 'legacy',
      settlement_kind: 'conservative',
      usage_measurement: {},
    },
  ]
  mount(
    <OperationsPage />,
    '/operations?tab=usage',
    mockApi((path) => (path.includes('/operations/usage') ? json({ data: rows, total: 3 }) : undefined)),
  )
  const table = within(await screen.findByRole('region', { name: 'Usage' }))
  const unknown = within(table.getByText('unknown').closest('tr')!)
  expect(unknown.getByText('Missing price')).toBeInTheDocument()
  expect(unknown.getAllByText('—').length).toBeGreaterThan(1)
  const free = within(table.getByText('free').closest('tr')!)
  expect(free.getAllByText('0').length).toBeGreaterThan(0)
  expect(free.getByText('$0.000000')).toBeInTheDocument()
  const legacy = within(table.getByText('legacy').closest('tr')!)
  expect(legacy.getByText('Historical amount')).toBeInTheDocument()
  expect(legacy.getByText('Conservative estimate')).toBeInTheDocument()
  expect(legacy.getByText('$0.000123')).toBeInTheDocument()
})

it('explicit zero confirmation saves true while nonzero and unrelated edits preserve configuration and rates', async () => {
  const { default: ModelsPage } = await import('./ModelsPage')
  const { fireEvent, waitFor } = await import('@testing-library/react')
  const { default: userEvent } = await import('@testing-library/user-event')
  const models = [
    {
      id: 'zero',
      provider_id: 'c1',
      provider_name: 'Primary',
      public_name: 'zero-base',
      upstream_name: 'zero',
      input_price_micros: 0,
      output_price_micros: 0,
      pricing_configured: false,
      enabled: true,
    },
    {
      id: 'paid',
      provider_id: 'c1',
      provider_name: 'Primary',
      public_name: 'paid-base',
      upstream_name: 'paid',
      input_price_micros: 12,
      output_price_micros: 34,
      pricing_configured: true,
      enabled: true,
    },
  ]
  const fetchMock = mockApi((path, init) =>
    path.includes('/operations/models') && init?.method !== 'POST' ? json({ data: models, total: 2 }) : undefined,
  )
  const view = mount(<ModelsPage />, '/models', fetchMock)
  const table = within(await screen.findByRole('region', { name: i18n.t('models') }))
  await userEvent.click(within(table.getByText('zero-base').closest('tr')!).getByRole('button', { name: 'Edit' }))
  let dialog = within(screen.getByRole('dialog'))
  expect(dialog.getByText(i18n.t('pricingBaseHelp'))).toBeInTheDocument()
  await userEvent.click(dialog.getByRole('switch', { name: 'Confirm base pricing, including intentional zero' }))
  await userEvent.click(dialog.getByRole('button', { name: 'Save' }))
  await waitFor(() =>
    expect(
      fetchMock.mock.calls.some(
        ([, init]) =>
          init?.method === 'POST' &&
          JSON.parse(String(init.body)).id === 'zero' &&
          JSON.parse(String(init.body)).pricing_configured === true,
      ),
    ).toBe(true),
  )
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
  await userEvent.click(within(table.getByText('paid-base').closest('tr')!).getByRole('button', { name: 'Edit' }))
  dialog = within(screen.getByRole('dialog'))
  expect(dialog.getByRole('switch', { name: 'Confirm base pricing, including intentional zero' })).toBeChecked()
  expect(dialog.getByRole('switch', { name: 'Confirm base pricing, including intentional zero' })).toBeDisabled()
  fireEvent.change(dialog.getByRole('textbox', { name: i18n.t('publicModel') }), { target: { value: 'renamed-base' } })
  await userEvent.click(dialog.getByRole('button', { name: 'Save' }))
  await waitFor(() => {
    const call = fetchMock.mock.calls.find(
      ([, init]) => init?.method === 'POST' && JSON.parse(String(init.body)).id === 'paid',
    )!
    const body = JSON.parse(String(call[1]?.body))
    expect(body).toMatchObject({
      pricing_configured: true,
      input_price_micros: 12,
      output_price_micros: 34,
      public_name: 'renamed-base',
    })
  })
  view.unmount()
})
it('uses measured subtotals and per-execution affected counts independently of logical request counts', async () => {
  const { default: AnalyticsPage } = await import('./AnalyticsPage')
  const rows = [
    {
      dimension: 'm1',
      requests: 1,
      attempts: 2,
      errors: 0,
      input_tokens: 0,
      output_tokens: null,
      cost_micros: 0,
      cost_measured: true,
      missing_pricing_count: 1,
      incomplete_usage_count: 0,
      measured_cost_count: 1,
    },
  ]
  mount(
    <AnalyticsPage />,
    '/analytics',
    mockApi((path) => (path.includes('/analytics?') ? json({ data: rows }) : undefined)),
  )
  expect(await screen.findAllByText(i18n.t('partialPricing', { missing: 1, incomplete: 0 }))).not.toHaveLength(0)
  const row = within(
    (await screen.findByRole('region', { name: i18n.t('performanceBreakdown') })).querySelector('tbody tr')!,
  )
  expect(row.getByText('1')).toBeInTheDocument()
  expect(row.getByText('$0.000000')).toBeInTheDocument()
  expect(row.getByText('Measured cost attempts: 1')).toBeInTheDocument()
})
it('cost items load the exact project-scoped usage and price facts and keep each quantity flag independent', async () => {
  const fetchMock = mockApi((path) =>
    path.includes('/operations/cost-items')
      ? json({
          data: [
            {
              id: 'item-paid',
              usage_id: 'usage-paid',
              component_id: 'component-input',
              quantity: 0,
              subtotal_micros: 0,
            },
            {
              id: 'item-missing',
              usage_id: 'usage-missing',
              component_id: 'component-output',
              quantity: 0,
              subtotal_micros: 0,
            },
          ],
          total: 2,
        })
      : path.endsWith('/operations/usage/usage-paid')
        ? json({
            id: 'usage-paid',
            price_id: 'price-paid',
            pricing_status: 'priced',
            total_cost_micros: 0,
            usage_measurement: { version: 1, input_tokens: true, output_tokens: false },
          })
        : path.endsWith('/operations/usage/usage-missing')
          ? json({
              id: 'usage-missing',
              price_id: 'price-missing',
              pricing_status: 'missing_price',
              total_cost_micros: 0,
              usage_measurement: { version: 1, input_tokens: false, output_tokens: true },
            })
          : path.endsWith('/operations/prices/price-paid')
            ? json({ components: [{ id: 'component-input', kind: 'input' }] })
            : path.endsWith('/operations/prices/price-missing')
              ? json({ components: [{ id: 'component-output', kind: 'output' }] })
              : undefined,
  )
  mount(<OperationsPage />, '/operations?tab=costItems', fetchMock)
  const { waitFor } = await import('@testing-library/react')
  await waitFor(() => expect(screen.getByText('$0.000000')).toBeInTheDocument())
  const table = within(await screen.findByRole('region', { name: i18n.t('costItems') }))
  const paid = within(table.getByText('usage-paid').closest('tr')!)
  expect(await paid.findByText('0')).toBeInTheDocument()
  const missing = within(table.getByText('usage-missing').closest('tr')!)
  expect(await missing.findByText('0')).toBeInTheDocument()
  expect(missing.getByText('—')).toBeInTheDocument()
  expect(missing.getByText('Missing price')).toBeInTheDocument()
  expect(fetchMock.mock.calls.some(([path]) => String(path).endsWith('/p1/operations/usage/usage-paid'))).toBe(true)
})
it('qualifies historical amounts included in trace totals while leaving their unspecified counters unmeasured', async () => {
  const { TraceDetailPage } = await import('./OperationsPage')
  const { Route, Routes } = await import('react-router')
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
            executions: [],
            usage: [
              {
                execution_id: 'e1',
                input_tokens: 7,
                output_tokens: 8,
                total_cost_micros: 100,
                pricing_status: 'legacy',
                usage_measurement: {},
              },
            ],
            cost_items: [],
          })
        : undefined,
    ),
  )
  const summary = within(await screen.findByRole('region', { name: i18n.t('traceUsage') }))
  expect(summary.getByText('$0.000100')).toBeInTheDocument()
  expect(summary.getByText(i18n.t('pricingHistoricalIncluded', { count: 1 }))).toBeInTheDocument()
  expect(summary.getAllByText('—')).toHaveLength(5)
})
