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
  const table = within(await screen.findByRole('region', { name: i18n.t('costItems') }))
  await waitFor(() => expect(table.getByText('$0.000000')).toBeInTheDocument())
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

it('derived cost items require measured cache/reasoning dependencies and exact TTL evidence even when priced', async () => {
  const usage = {
    id: 'u-derived',
    execution_id: 'e-derived',
    price_id: 'price-derived',
    pricing_status: 'priced',
    cost_micros: 3,
    usage_measurement: {
      version: 1,
      input_tokens: true,
      output_tokens: true,
      cache_read_tokens: false,
      cache_write_tokens: true,
      reasoning_tokens: false,
    },
  }
  const components = [
    { id: 'uncached', kind: 'input', unit_price_micros: 0, cache_ttl: null },
    { id: 'cache', kind: 'cache_read', unit_price_micros: 0, cache_ttl: null },
    { id: 'ordinary-output', kind: 'output', unit_price_micros: 0, cache_ttl: null },
    { id: 'thinking', kind: 'reasoning', unit_price_micros: 0, cache_ttl: null },
    { id: 'write-5m', kind: 'cache_write', unit_price_micros: 0, cache_ttl: '5m' },
    { id: 'write-1h', kind: 'cache_write', unit_price_micros: 0, cache_ttl: '1h' },
    { id: 'fixed-charge', kind: 'flat', unit_price_micros: 3, cache_ttl: null },
  ]
  const items = components.map((component, index) => ({
    id: component.id,
    usage_id: 'u-derived',
    component_id: component.id,
    quantity: index === 0 ? 6 : index === 2 ? 3 : index === 4 ? 4 : index === 6 ? 1 : 0,
    subtotal_micros: index === 6 ? 3 : 0,
  }))
  mount(
    <OperationsPage />,
    '/operations?tab=costItems',
    mockApi((path) =>
      path.includes('/operations/cost-items')
        ? json({ data: items, total: items.length })
        : path.endsWith('/operations/usage/u-derived')
          ? json(usage)
          : path.endsWith('/operations/prices/price-derived')
            ? json({ components })
            : undefined,
    ),
  )
  const table = within(await screen.findByRole('region', { name: i18n.t('costItems') }))
  for (const component of components) {
    const row = within(table.getByText(component.id).closest('tr')!)
    const { waitFor } = await import('@testing-library/react')
    await waitFor(() => expect(row.getAllByRole('cell')[2]).toHaveTextContent('—'))
    expect(row.getAllByRole('cell')[3]).toHaveTextContent(component.id === 'fixed-charge' ? '$0.000003' : '$0.000000')
  }
})
it.each(['trace', 'request'])(
  'preserves gross counters and known charges while derived %s quantities stay unknown',
  async (view) => {
    const { TraceDetailPage, RequestDetailPage } = await import('./OperationsPage')
    const { Route, Routes } = await import('react-router')
    const usage = {
      execution_id: 'e1',
      input_tokens: 10,
      output_tokens: 3,
      cache_read_tokens: 0,
      cache_write_tokens: 4,
      reasoning_tokens: 0,
      total_cost_micros: 3,
      pricing_status: 'priced',
      usage_measurement: {
        version: 1,
        input_tokens: true,
        output_tokens: true,
        cache_read_tokens: false,
        cache_write_tokens: true,
        reasoning_tokens: false,
      },
    }
    const cost_items = [
      { execution_id: 'e1', kind: 'input', quantity: 6, unit_price_micros: 0, subtotal_micros: 0 },
      { execution_id: 'e1', kind: 'cache_read', quantity: 0, unit_price_micros: 0, subtotal_micros: 0 },
      { execution_id: 'e1', kind: 'output', quantity: 3, unit_price_micros: 0, subtotal_micros: 0 },
      { execution_id: 'e1', kind: 'reasoning', quantity: 0, unit_price_micros: 0, subtotal_micros: 0 },
      { execution_id: 'e1', kind: 'cache_write', quantity: 4, unit_price_micros: 0, subtotal_micros: 0 },
      { execution_id: 'e1', kind: 'flat', quantity: 1, unit_price_micros: 3, subtotal_micros: 3 },
    ]
    const record = {
      trace: { id: 't1', started_at: 1, finished_at: 2, status: 'succeeded' },
      requests: [],
      executions: [{ id: 'e1', attempt: 1, status: 'succeeded' }],
      usage: [usage],
      cost_items,
    }
    const detail = {
      ...usage,
      id: 'r1',
      request_id: 'public-r1',
      cost_micros: 3,
      latency_ms: 1,
      started_at: 1,
      status_code: 200,
      trace_id: null,
      cached_tokens: 0,
    }
    mount(
      <Routes>
        <Route
          path={`/operations/${view === 'trace' ? 'traces' : 'requests'}/:id`}
          element={view === 'trace' ? <TraceDetailPage /> : <RequestDetailPage />}
        />
      </Routes>,
      `/operations/${view === 'trace' ? 'traces/t1' : 'requests/r1'}`,
      mockApi((path) =>
        path.includes('/trace-detail')
          ? json(record)
          : path.includes('/observability/requests/r1')
            ? json(detail)
            : path.endsWith('/operations/requests/r1')
              ? json(record)
              : undefined,
      ),
    )
    await screen.findByText(i18n.t(view === 'trace' ? 'traceUsage' : 'usageBreakdown'))
    const costTable =
      view === 'trace'
        ? within(screen.getByRole('region', { name: i18n.t('costComponents') })).getByRole('table')
        : screen.getByRole('columnheader', { name: i18n.t('costComponents') }).closest('table')!
    const costRows = within(costTable).getAllByRole('row').slice(1)
    for (const row of costRows) {
      expect(within(row).getAllByRole('cell')[view === 'trace' ? 2 : 1]).toHaveTextContent('—')
    }
    const fixed = within(within(costTable).getByText(i18n.t('costKindFlat')).closest('tr')!)
    expect(fixed.getAllByRole('cell')[view === 'trace' ? 4 : 2]).toHaveTextContent('$0.000003')
    expect(screen.getAllByText('10').length).toBeGreaterThan(0)
  },
)
it('Overview retains legacy-inclusive Summary money without claiming confirmed pricing coverage for stats or trend', async () => {
  const { default: OverviewPage } = await import('./OverviewPage')
  const bucket = 1700000000
  const point = {
    bucket,
    requests: 1,
    errors: 0,
    latency_ms: 1,
    input_tokens: null,
    output_tokens: null,
    cost_micros: 99,
    missing_pricing_count: 0,
    incomplete_usage_count: 0,
    measured_cost_count: 1,
  }
  const summary = { ...point, error_rate: 0, p95_latency_ms: 1, series: [point] }
  mount(
    <OverviewPage />,
    '/',
    mockApi((path) =>
      path.includes('/api/v1/bootstrap')
        ? json({ observability_available: true })
        : path.includes('/settings/request-logging')
          ? json({ enabled: true, default_level: 'metadata' })
          : path.includes('/observability/summary')
            ? json(summary)
            : path.includes('/observability/requests')
              ? json({ data: [], total: 0 })
              : undefined,
    ),
  )
  const { waitFor } = await import('@testing-library/react')
  await waitFor(() => expect(document.querySelector('[data-stat="cost"]')).toHaveTextContent('$0.000099'))
  expect(
    await screen.findAllByText('Subtotal may include historical amounts; their pricing coverage is unmeasured.'),
  ).not.toHaveLength(0)
  expect(screen.queryByText('Measured cost attempts: 1')).not.toBeInTheDocument()
  expect(screen.getByRole('columnheader', { name: i18n.t('cost') }).closest('table')).toHaveTextContent('$0.000099')
  expect(screen.getByRole('columnheader', { name: i18n.t('cost') }).closest('table')).toHaveTextContent(
    'Subtotal may include historical amounts',
  )
})
it('retains fully reported derived input/output quantities and exact total cache-write quantities at zero cost', async () => {
  const components = [
    { id: 'input-component', kind: 'input', cache_ttl: null },
    { id: 'read-component', kind: 'cache_read', cache_ttl: null },
    { id: 'write-component', kind: 'cache_write', cache_ttl: null },
    { id: 'output-component', kind: 'output', cache_ttl: null },
    { id: 'reason-component', kind: 'reasoning', cache_ttl: null },
  ]
  const quantities = [6, 2, 2, 2, 1]
  const items = components.map((c, index) => ({
    id: c.id,
    usage_id: 'known-usage',
    component_id: c.id,
    quantity: quantities[index],
    subtotal_micros: 0,
  }))
  const usage = {
    id: 'known-usage',
    price_id: 'known-price',
    pricing_status: 'explicit_free',
    cost_micros: 0,
    usage_measurement: {
      version: 1,
      input_tokens: true,
      output_tokens: true,
      cache_read_tokens: true,
      cache_write_tokens: true,
      reasoning_tokens: true,
    },
  }
  mount(
    <OperationsPage />,
    '/operations?tab=costItems',
    mockApi((path) =>
      path.includes('/operations/cost-items')
        ? json({ data: items, total: items.length })
        : path.endsWith('/operations/usage/known-usage')
          ? json(usage)
          : path.endsWith('/operations/prices/known-price')
            ? json({ components })
            : undefined,
    ),
  )
  const table = within(await screen.findByRole('region', { name: i18n.t('costItems') }))
  for (const [c, index] of components.map((c, index) => [c, index] as const)) {
    const row = within(table.getByText(c.id).closest('tr')!)
    expect(await row.findByText(String(quantities[index]))).toBeInTheDocument()
    expect(row.getByText('$0.000000')).toBeInTheDocument()
  }
})
it('unresolved sibling kinds cannot establish input/output splits while independent cache counts and charges remain visible', async () => {
  const { TraceDetailPage } = await import('./OperationsPage')
  const { Route, Routes } = await import('react-router')
  const cost_items = [
    { execution_id: 'e1', kind: 'input', quantity: 8, unit_price_micros: 0, subtotal_micros: 0 },
    { execution_id: 'e1', kind: 'output', quantity: 3, unit_price_micros: 0, subtotal_micros: 0 },
    { execution_id: 'e1', kind: 'cache_read', quantity: 2, unit_price_micros: 0, subtotal_micros: 0 },
    { execution_id: 'e1', kind: null, quantity: 1, unit_price_micros: null, subtotal_micros: 99 },
  ]
  const usage = {
    execution_id: 'e1',
    input_tokens: 10,
    output_tokens: 3,
    cache_read_tokens: 2,
    total_cost_micros: 99,
    pricing_status: 'legacy',
    usage_measurement: { version: 1, input_tokens: true, output_tokens: true, cache_read_tokens: true },
  }
  mount(
    <Routes>
      <Route path="/operations/traces/:id" element={<TraceDetailPage />} />
    </Routes>,
    '/operations/traces/t1',
    mockApi((path) =>
      path.includes('/trace-detail')
        ? json({
            trace: { id: 't1', started_at: 1, status: 'succeeded' },
            requests: [],
            executions: [{ id: 'e1', attempt: 1, status: 'succeeded' }],
            usage: [usage],
            cost_items,
          })
        : undefined,
    ),
  )
  const table = within(await screen.findByRole('region', { name: i18n.t('costComponents') }))
  for (const label of ['costKindInput', 'costKindOutput']) {
    const row = within(table.getByText(i18n.t(label)).closest('tr')!)
    expect(row.getAllByRole('cell')[2]).toHaveTextContent('—')
  }
  const cache = within(table.getByText(i18n.t('costKindCacheRead')).closest('tr')!)
  expect(cache.getAllByRole('cell')[2]).toHaveTextContent('2')
  expect(table.getByText('$0.000099')).toBeInTheDocument()
})
