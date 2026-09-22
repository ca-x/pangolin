import { useQuery } from '@tanstack/react-query'
import { Activity, CircleDollarSign, Clock3, Server, ShieldCheck } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router'
import { api, type AnalyticsRow, type Paged, type RequestItem, type Summary } from '../api'
import { ANALYTICS_DIMENSIONS, ANALYTICS_DIMENSION_KEYS, type AnalyticsDimension, useAnalyticsDimensionNames } from '../analyticsDimensions'
import { EmptyState, SkeletonRows, Status } from '../components'
import i18n from '../i18n'
import { ObservabilityNotice, UNMEASURED, formatCount, formatMicros, useObservability } from '../observability'
import { PageHeader, QueryError, displayValue, formatDate } from './shared'
import { useProject } from '../project'
import { Button, Card, Group, Select, SimpleGrid, Stack, Table, TableScrollContainer, Text, ThemeIcon, Title } from '@mantine/core'

const formatNumber = (value: number) => new Intl.NumberFormat(i18n.language, { notation: value > 9999 ? 'compact' : 'standard', maximumFractionDigits: 1 }).format(value)
const formatShortDate = (value: number) => new Intl.DateTimeFormat(i18n.language, { month: 'numeric', day: 'numeric', hour: 'numeric' }).format(new Date(value * 1000))
/** A daily bucket has no hour to name: printing `12 AM` beside every one of them
 * is noise, so a bucket a day wide or wider is labelled by its date. */
const formatBucket = (value: number, width: number) => width >= 86_400
  ? new Intl.DateTimeFormat(i18n.language, { month: 'numeric', day: 'numeric' }).format(new Date(value * 1000))
  : formatShortDate(value)

/**
 * The windows the overview offers. Every preset is resolved once, when the operator
 * picks it, into one half-open event-time window (`[from, until)` — the same
 * convention the summary and the breakdown read with), so a retry re-reads the same
 * bounds instead of sliding them forward under the reader.
 */
const WINDOWS = ['24h', '7d', '30d', 'all'] as const
type WindowKey = typeof WINDOWS[number]
const WINDOW_SECONDS: Record<Exclude<WindowKey, 'all'>, number> = { '24h': 86_400, '7d': 604_800, '30d': 2_592_000 }
/**
 * One stable `{from, until}` pair per selection, passed to both the summary and
 * the breakdown so the cards and the table describe the same events. The pair is
 * half-open on event time — `from` is in the window, `until` is not — which is the
 * convention both endpoints read with, so an event admitted in the second the
 * window ends belongs to the next window rather than to neither or to both. `all`
 * starts at the beginning of time: the projection's own retention is the ultimate
 * bound, so the answer stays an aggregate over retained buckets rather than an
 * unbounded read of every request.
 */
const windowBounds = (window: WindowKey, now: number) => window === 'all'
  ? { from: 0, until: now }
  : { from: now - WINDOW_SECONDS[window], until: now }

/**
 * The selected window's name in the active language. The header, the stat group's
 * accessible name, the empty state, the breakdown hint and the selector all read
 * it, so one selection renames every surface that claims a window.
 */
const useWindowLabels = () => {
  const { t } = useTranslation()
  return useMemo(
    () => ({ '24h': t('last24h'), '7d': t('last7d'), '30d': t('last30d'), all: t('allRetained') } as Record<WindowKey, string>),
    [t],
  )
}
/** Round an axis step up to a readable 1 / 1.5 / 2 / 2.5 / 3 / 4 / 5 / 6 / 8 / 10 ladder. */
const niceStep = (value: number) => {
  const magnitude = 10 ** Math.floor(Math.log10(Math.max(value, 1)))
  const normalized = value / magnitude
  const step = [1, 1.5, 2, 2.5, 3, 4, 5, 6, 8, 10].find((candidate) => normalized <= candidate) ?? 10
  return step * magnitude
}

export default function OverviewPage() {
  const { t } = useTranslation()
  const { project } = useProject()
  const observability = useObservability()
  const labels = useWindowLabels()
  const [window, setWindow] = useState<WindowKey>('24h')
  // Resolved once, when the operator picks a window: a retry re-reads the same
  // bounds rather than a window that slid forward while the read was failing.
  const [bounds, setBounds] = useState(() => windowBounds('24h', Math.floor(Date.now() / 1000)))
  const choose = (next: WindowKey) => { setWindow(next); setBounds(windowBounds(next, Math.floor(Date.now() / 1000))) }
  const base = `/api/admin/v1/projects/${encodeURIComponent(project.id)}/observability`
  const summary = useQuery({ queryKey: ['summary', project.id, bounds.from, bounds.until], queryFn: () => api<Summary>(`${base}/summary?from=${bounds.from}&until=${bounds.until}`) })
  // The recent list is a newest-six read, not a windowed one, and does not claim
  // to be: it is named for what it is on the card and in its empty state.
  const requests = useQuery({ queryKey: ['requests', project.id, 'recent'], queryFn: () => api<Paged<RequestItem>>(`${base}/requests?limit=6`) })
  const value = summary.data
  // A zero is only a fact when telemetry is actually recorded: with the
  // projection down, or with request logging `off`, the same zero means
  // "nothing was written", and every figure on this page is unknown.
  const measured = observability.measured
  const retry = () => { void summary.refetch(); void requests.refetch(); observability.refresh() }
  const header = <PageHeader
    title={t('overview')}
    description={labels[window]}
    action={<Select aria-label={t('timeWindow')} data={WINDOWS.map((value) => ({ value, label: labels[value] }))} value={window} onChange={(next) => next && choose(next as WindowKey)} allowDeselect={false} w={{ base: '100%', xs: 200 }} />}
  />
  if ((summary.isError || requests.isError) && measured) return <>{header}<QueryError retry={retry} /></>
  // Token and cost totals are unmeasured when the window settled no usage at all:
  // `—` there, never an invented zero, and a measured zero stays a real zero.
  const tokens = value && value.input_tokens != null && value.output_tokens != null ? value.input_tokens + value.output_tokens : null
  const cost = value ? value.cost_micros : null
  // The backend counts failed requests itself; reconstructing the count from the
  // rounded rate disagreed with the row-level statuses.
  const failed = value ? value.errors : null
  const hasTraffic = Boolean(value && value.requests > 0)
  return <>{header}
    <ObservabilityNotice observability={observability} onRetry={retry} />
    <SimpleGrid cols={{ base: 1, xs: 2, md: 4 }} mb="lg" role="group" aria-label={labels[window]}>
      <Stat id="requests" icon={<Activity />} label={t('totalRequests')} value={measured && value ? formatNumber(value.requests) : UNMEASURED} sub={measured && tokens != null && tokens > 0 ? `${formatNumber(tokens)} ${t('tokens')}` : undefined} />
      <Stat id="errors" icon={<ShieldCheck />} label={t('errorRate')} value={measured && hasTraffic && value!.error_rate != null ? <>{(value!.error_rate * 100).toFixed(1)}<Text component="span" fz=".62em" fw={500} c="dimmed">%</Text></> : UNMEASURED} sub={measured && failed ? t('failedRequests', { count: failed }) : undefined} tone={measured && value?.error_rate != null && value.error_rate > .05 ? 'danger' : undefined} />
      <Stat id="latency" icon={<Clock3 />} label={t('p95Latency')} value={measured && hasTraffic && value!.p95_latency_ms != null ? <>{Math.round(value!.p95_latency_ms)}<Text component="span" fz=".62em" fw={500} c="dimmed"> ms</Text></> : UNMEASURED} />
      <Stat id="cost" icon={<CircleDollarSign />} label={t('cost')} value={measured && value ? formatMicros(cost) : UNMEASURED} sub={measured && hasTraffic && cost != null ? `${formatMicros(cost / value!.requests)} ${t('perRequest')}` : undefined} />
    </SimpleGrid>
    {!measured ? null : summary.isLoading || requests.isLoading ? <SkeletonRows count={5}/> : value?.requests === 0 ? <EmptyState icon={<Server />} title={t('overviewEmptyTitle', { window: labels[window] })} copy={t('overviewEmptyCopy')} action={<Button component={Link} to="/channels">{t('configureChannel')}</Button>} /> : <div className="overview-grid"><Card p="lg"><Group justify="space-between" align="flex-start" mb="sm"><Stack gap={2}><Title order={2}>{t('requestTrend')}</Title><Text size="sm" c="dimmed">{`${formatNumber(value?.requests || 0)} ${t('requests')} · ${t('tokens')}: ${formatCount(tokens)}`}</Text></Stack><ChartLegend /></Group><TrendChart series={value?.series || []}/><details className="chart-table"><summary>{t('dataTable')}</summary><table><thead><tr><th>{t('startedAt')}</th><th>{t('requests')}</th><th>{t('errorsLabel')}</th><th>{t('inputTokens')}</th><th>{t('outputTokens')}</th><th>{t('totalTokens')}</th><th>{t('cost')}</th></tr></thead><tbody>{value?.series.map((point) => <tr key={point.bucket}><td>{formatDate(point.bucket)}</td><td className="mono-cell">{formatCount(point.requests)}</td><td className="mono-cell">{formatCount(point.errors)}</td><td className="mono-cell">{formatCount(point.input_tokens)}</td><td className="mono-cell">{formatCount(point.output_tokens)}</td><td className="mono-cell">{formatCount(totalTokens(point))}</td><td className="mono-cell">{formatMicros(point.cost_micros)}</td></tr>)}</tbody></table></details></Card><Card p="lg"><Group justify="space-between" align="flex-start" mb="sm"><Title order={2}>{t('recentRequests')}</Title><Button variant="subtle" size="compact-sm" component={Link} to="/operations">{t('viewAll')}</Button></Group><CompactRequests rows={requests.data?.data || []}/></Card></div>}
    {measured && hasTraffic && <Breakdowns projectId={project.id} bounds={bounds} windowLabel={labels[window]} />}
  </>
}

function Stat({ id, icon, label, value, sub, tone }: { id: string; icon: React.ReactNode; label: string; value: React.ReactNode; sub?: string; tone?: 'danger' }) {
  return <Card p="md">
    <Group gap="sm" align="flex-start" wrap="nowrap">
      <ThemeIcon variant="light" size="lg" color={tone === 'danger' ? 'red' : undefined}>{icon}</ThemeIcon>
      <Stack gap={2} style={{ minWidth: 0 }}>
        <Text size="xs" c="dimmed" fw={540}>{label}</Text>
        <Text component="span" fz="1.6rem" fw={620} lh="1.15" className="stat-value" data-stat={id}>{value}</Text>
        {sub && <Text size="xs" c="dimmed" className="stat-sub">{sub}</Text>}
      </Stack>
    </Group>
  </Card>
}

function ChartLegend() {
  const { t } = useTranslation()
  // The legend names the two series drawn on the count scale. Tokens and cost are
  // separate rows below, each labelled with its own unit and range, so they need
  // no second accent hue — the console keeps one accent per screen.
  const entries: Array<[string, string, boolean]> = [
    [t('requests'), 'var(--accent)', false],
    [t('errorsLabel'), 'var(--mantine-color-red-6)', true],
  ]
  return <Group gap="sm" wrap="nowrap" aria-hidden="true">
    {entries.map(([label, color, dashed]) => <Group gap={6} wrap="nowrap" key={label}>
      <span className="chart-legend-swatch" style={{ width: 10, height: 3, borderRadius: 2, background: color, ...(dashed ? { backgroundImage: `repeating-linear-gradient(90deg, ${color} 0 4px, transparent 4px 7px)` } : {}) }} />
      <Text size="xs" c="dimmed">{label}</Text>
    </Group>)}
  </Group>
}

type SeriesPoint = Summary['series'][number]
/** A value the projection may not have measured. `null` is a gap, not a zero. */
type Measured = number | null

const CHART_WIDTH = 640
const CHART_HEIGHT = 240
const SPARK_HEIGHT = 56
const HOUR = 3600

/** The readable ladder above a series' own largest measured value. */
const scaleMax = (values: number[]) => Math.max(1, niceStep((Math.max(...values, 0) * 1.08) / 4) * 4)
/** Total tokens of a bucket, or `null` when either half was not measured. */
const totalTokens = (point: SeriesPoint): Measured => point.input_tokens == null || point.output_tokens == null ? null : point.input_tokens + point.output_tokens
/**
 * Runs of consecutive measured buckets. A gap breaks the line instead of dropping
 * to zero — an unmeasured bucket is not a collapse — and an isolated measurement
 * keeps its dot without a slope the record never reported.
 */
const measuredRuns = (values: Measured[]) => {
  const runs: Array<Array<{ index: number; value: number }>> = []
  let run: Array<{ index: number; value: number }> = []
  values.forEach((value, index) => {
    if (value == null) { if (run.length) runs.push(run); run = []; return }
    run.push({ index, value })
  })
  if (run.length) runs.push(run)
  return runs
}
/**
 * The path of one series. A single bucket is one measurement, not a slope: anchored
 * at x=0 the line closed to the bottom-right corner and drew a triangle down to
 * zero, which is a collapse the backend never reported, so one bucket is drawn as a
 * level line across the plot. An unmeasured series draws nothing at all.
 */
const seriesPath = (values: Measured[], flat: boolean, x: (index: number) => number, y: (value: number) => number) => {
  if (flat) return values[0] == null ? '' : `M ${(0).toFixed(2)} ${y(values[0]).toFixed(2)} L ${CHART_WIDTH.toFixed(2)} ${y(values[0]).toFixed(2)}`
  return measuredRuns(values).map((run) => run.length === 1
    ? `M ${x(run[0].index).toFixed(2)} ${y(run[0].value).toFixed(2)}`
    : run.map((point, index) => `${index ? 'L' : 'M'} ${x(point.index).toFixed(2)} ${y(point.value).toFixed(2)}`).join(' ')).join(' ')
}

function TrendChart({ series }: { series: SeriesPoint[] }) {
  const { t } = useTranslation()
  const [active, setActive] = useState<number | null>(null)
  if (!series.length) return <div className="chart-empty-line" />
  // One bucket is one measurement, not a slope: it is drawn level across the plot
  // with its dot in the middle; two or more keep their own event-time positions.
  const flat = series.length === 1
  const x = (index: number) => flat ? CHART_WIDTH / 2 : (index / Math.max(series.length - 1, 1)) * CHART_WIDTH
  // Requests and errors share the count scale. Tokens and cost have their own
  // scales below, so no unrelated unit is normalised onto this axis.
  const countMax = scaleMax(series.map((point) => point.requests))
  const y = (count: number) => (1 - count / countMax) * CHART_HEIGHT
  const line = seriesPath(series.map((point) => point.requests), flat, x, y)
  const area = `${line} L ${CHART_WIDTH} ${CHART_HEIGHT} L 0 ${CHART_HEIGHT} Z`
  // Failed requests are counted per bucket by the backend, so the error line is
  // read from the series rather than reconstructed from an aggregate rate.
  const errorLine = seriesPath(series.map((point) => point.errors), flat, x, y)
  const gridlines = [0, .25, .5, .75, 1]
  const firstBucket = series[0].bucket
  const lastBucket = series[series.length - 1].bucket
  const span = Math.max(lastBucket - firstBucket, 1)
  // The labels follow the buckets the backend chose: a day of hourly buckets is
  // labelled every few hours, a month of daily ones every few days.
  const bucketWidth = series.length > 1 ? Math.max(series[1].bucket - series[0].bucket, 1) : HOUR
  const tickStep = Math.max(bucketWidth, Math.ceil(span / 6 / bucketWidth) * bucketWidth)
  const tickTimes: number[] = []
  for (let time = Math.ceil(firstBucket / tickStep) * tickStep; time <= lastBucket; time += tickStep) tickTimes.push(time)
  if (!tickTimes.length) tickTimes.push(firstBucket)
  const focus = active == null ? null : { x: x(active), y: y(series[active].requests), ...series[active] }
  const inspect = (event: React.MouseEvent<SVGSVGElement>) => {
    const bounds = event.currentTarget.getBoundingClientRect()
    const ratio = (event.clientX - bounds.left) / bounds.width
    const index = Math.round(ratio * (series.length - 1))
    setActive(Math.min(Math.max(index, 0), series.length - 1))
  }
  // A generic `div` has no role that permits a name, so axe reported
  // `aria-prohibited-attr` and the hint never reached the accessibility tree. The
  // chart is one graphic and everything inside it is already `aria-hidden`; the
  // disclosure table beside it is the accessible surface.
  return <div className="chart-wrap" role="img" aria-label={t('requestTrendHint')}>
    <div className="chart-plot">
      <div className="chart-y-labels" aria-hidden="true">{gridlines.map((ratio) => <span key={ratio} className="chart-axis-label" style={{ top: `${ratio * 100}%` }}>{formatNumber(Math.round(countMax * (1 - ratio)))}</span>)}</div>
      <div className="chart-canvas">
        <svg className="trend-chart" viewBox={`0 0 ${CHART_WIDTH} ${CHART_HEIGHT}`} preserveAspectRatio="none" aria-hidden="true" onMouseMove={inspect} onMouseLeave={() => setActive(null)}>
          <defs><linearGradient id="pangolin-trend-fill" x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stopColor="var(--accent)" stopOpacity=".26"/><stop offset="100%" stopColor="var(--accent)" stopOpacity="0"/></linearGradient></defs>
          {gridlines.map((ratio) => <line key={ratio} className="chart-grid-line" x1="0" x2={CHART_WIDTH} y1={ratio * CHART_HEIGHT} y2={ratio * CHART_HEIGHT} vectorEffect="non-scaling-stroke"/>)}
          <path className="chart-area" d={area} vectorEffect="non-scaling-stroke"/>
          <path className="chart-line" d={line} vectorEffect="non-scaling-stroke"/>
          <path className="chart-line" d={errorLine} vectorEffect="non-scaling-stroke" stroke="var(--mantine-color-red-6)" strokeDasharray="4 3" fill="none"/>
          {/* deslop-ignore-next-line 24 — chart marks, not icons */}
          {series.length <= 14 && series.map((point, index) => <circle key={point.bucket} className="chart-dot" cx={x(index)} cy={y(point.requests)} r="3" vectorEffect="non-scaling-stroke"/>)}
          {focus && <line className="chart-crosshair" x1={focus.x} x2={focus.x} y1="0" y2={CHART_HEIGHT} vectorEffect="non-scaling-stroke"/>}
          {/* deslop-ignore-next-line 24 — chart marks, not icons */}
          {focus && <circle className="chart-dot-active" cx={focus.x} cy={focus.y} r="4.5" vectorEffect="non-scaling-stroke"/>}
        </svg>
        {focus && <div className="chart-tooltip" style={{ left: `${(focus.x / CHART_WIDTH) * 100}%`, top: `${(focus.y / CHART_HEIGHT) * 100}%` }}><span>{formatBucket(focus.bucket, bucketWidth)}</span><div><strong>{formatNumber(focus.requests)}</strong><span>{t('requests')}</span></div><div><strong>{formatNumber(focus.errors)}</strong><span>{t('errorsLabel')}</span></div><div><strong>{formatCount(focus.input_tokens)}</strong><span>{t('inputTokens')}</span></div><div><strong>{formatCount(focus.output_tokens)}</strong><span>{t('outputTokens')}</span></div><div><strong>{formatCount(totalTokens(focus))}</strong><span>{t('totalTokens')}</span></div><div><strong>{formatMicros(focus.cost_micros)}</strong><span>{t('cost')}</span></div></div>}
      </div>
    </div>
    <SparkRow label={t('totalTokens')} values={series.map(totalTokens)} format={formatNumber} flat={flat} x={x} focus={focus}/>
    <SparkRow label={t('cost')} values={series.map((point) => point.cost_micros)} format={formatMicros} flat={flat} x={x} focus={focus}/>
    <div className="chart-x-labels" aria-hidden="true">{tickTimes.map((time) => <span key={time} className="chart-axis-label" style={{ left: `${((time - firstBucket) / span) * 100}%` }}>{formatBucket(time, bucketWidth)}</span>)}</div>
  </div>
}

/**
 * One unit on its own scale, sharing the event-time x positions of the plot above.
 * The head names the unit and its range, so a reader never has to guess which axis
 * a line belongs to. A series with nothing measured says so instead of drawing a
 * flat line at zero, and a measured zero is plotted as the zero it is.
 */
function SparkRow({ label, values, format, flat, x, focus }: { label: string; values: Measured[]; format: (value: number) => string; flat: boolean; x: (index: number) => number; focus: { x: number } | null }) {
  const measured = values.filter((value): value is number => value != null)
  const max = scaleMax(measured)
  const y = (value: number) => (1 - value / max) * SPARK_HEIGHT
  return <div className="chart-spark">
    <div className="chart-spark-head" aria-hidden="true">
      <span className="chart-spark-label">{label}</span>
      <span className="chart-spark-scale">{measured.length ? `0 – ${format(max)}` : UNMEASURED}</span>
    </div>
    <svg className="trend-spark" viewBox={`0 0 ${CHART_WIDTH} ${SPARK_HEIGHT}`} preserveAspectRatio="none" aria-hidden="true">
      <line className="chart-grid-line" x1="0" x2={CHART_WIDTH} y1={SPARK_HEIGHT} y2={SPARK_HEIGHT} vectorEffect="non-scaling-stroke"/>
      <path className="chart-spark-line" d={seriesPath(values, flat, x, y)} vectorEffect="non-scaling-stroke"/>
      {/* deslop-ignore-next-line 24 — chart marks, not icons */}
      {values.length <= 14 && values.map((value, index) => value == null ? null : <circle key={index} className="chart-spark-dot" cx={x(index)} cy={y(value)} r="2.5" vectorEffect="non-scaling-stroke"/>)}
      {focus && <line className="chart-crosshair" x1={focus.x} x2={focus.x} y1="0" y2={SPARK_HEIGHT} vectorEffect="non-scaling-stroke"/>}
    </svg>
  </div>
}

function CompactRequests({ rows }: { rows: RequestItem[] }) {
  const { t } = useTranslation()
  // The newest-six list is not window-filtered, so its empty state must not name a
  // window the operator may not have selected.
  if (!rows.length) return <EmptyState icon={<Activity/>} title={t('recentRequests')} copy={t('noRecentRequests')}/>
  // Same identity rule as the request log: the row is keyed and linked by Pangolin's
  // own UUID, because the caller-chosen external id repeats.
  return <div className="compact-list">{rows.map((row) => <Link to={`/operations/requests/${row.internal_id}`} key={row.internal_id}><Status code={row.status_code} /><Text fw={560}>{displayValue(row.requested_model)}</Text><small>{row.latency_ms >= 0 ? `${row.latency_ms} ms` : UNMEASURED}</small></Link>)}</div>
}

/**
 * Where the traffic of the window went, one row per channel, model, key, user or
 * project. The endpoint reports counts, tokens, cost and averages; an average it
 * could not compute stays `—`, and so does a dimension value the projection
 * recorded as null, while a measured zero prints as `0`.
 */
function Breakdowns({ projectId, bounds, windowLabel }: { projectId: string; bounds: { from: number; until: number }; windowLabel: string }) {
  const { t } = useTranslation()
  const [dimension, setDimension] = useState<AnalyticsDimension>('provider')
  const query = useQuery({
    queryKey: ['analytics', projectId, dimension, bounds.from, bounds.until],
    // The cards above state a window, so the breakdown is read over the same
    // bounds — not the endpoint's own default of the last 30 days, and not a
    // window of its own that would disagree with them.
    queryFn: () => api<{ data: AnalyticsRow[] }>(`/api/admin/v1/projects/${encodeURIComponent(projectId)}/analytics?dimension=${dimension}&from=${bounds.from}&until=${bounds.until}`),
  })
  const names = useAnalyticsDimensionNames(projectId, dimension)
  const rows = query.data?.data ?? []
  return <Card p="lg" mt="lg">
    <Group justify="space-between" align="flex-end" mb="sm" wrap="wrap">
      <Stack gap={2}><Title order={2}>{t('breakdowns')}</Title><Text size="sm" c="dimmed">{t('breakdownsHint', { window: windowLabel })}</Text></Stack>
      <Select label={t('dimension')} data={ANALYTICS_DIMENSIONS.map((value) => ({ value, label: t(ANALYTICS_DIMENSION_KEYS[value]) }))} value={dimension} onChange={(value) => value && setDimension(value as AnalyticsDimension)} w={{ base: '100%', xs: 220 }} />
    </Group>
    {query.isError ? <QueryError retry={() => void query.refetch()} /> : query.isLoading ? <SkeletonRows count={3} /> : !rows.length ? <EmptyState icon={<Server />} title={t('breakdowns')} copy={t('breakdownsEmpty')} /> : <>
      {/* The breakdown is real data even when its labels are not readable, so a
          failed lookup degrades to the ids and offers the retry instead of
          hiding rows the operator asked for. */}
      {names.query.isError && <Stack mb="sm"><QueryError retry={() => void names.query.refetch()} /></Stack>}
      <TableScrollContainer minWidth={860} role="region" aria-label={t('breakdowns')} tabIndex={0} style={{ maxHeight: 'min(60vh, 720px)' }}>
        <Table stickyHeader highlightOnHover>
          <Table.Thead><Table.Tr><Table.Th>{t('dimension')}</Table.Th><Table.Th>{t('requests')}</Table.Th><Table.Th>{t('errorsLabel')}</Table.Th><Table.Th>{t('inputTokens')}</Table.Th><Table.Th>{t('outputTokens')}</Table.Th><Table.Th>{t('cacheTokens')}</Table.Th><Table.Th>{t('cost')}</Table.Th><Table.Th>{t('latency')}</Table.Th></Table.Tr></Table.Thead>
          <Table.Tbody>{rows.map((row, index) => {
            const cell = names.resolve(row.dimension)
            return <Table.Tr key={`${row.dimension ?? 'unrecorded'}-${index}`}>
              <Table.Td className={cell.opaque ? 'mono-cell' : undefined}>{cell.text}</Table.Td>
              <Table.Td className="mono-cell">{formatCount(row.requests)}</Table.Td>
              <Table.Td className="mono-cell">{formatCount(row.errors)}</Table.Td>
              <Table.Td className="mono-cell">{formatCount(row.input_tokens)}</Table.Td>
              <Table.Td className="mono-cell">{formatCount(row.output_tokens)}</Table.Td>
              <Table.Td className="mono-cell">{formatCount(row.cache_hit_tokens)}</Table.Td>
              <Table.Td className="mono-cell">{formatMicros(row.cost_micros)}</Table.Td>
              <Table.Td className="mono-cell">{row.latency_ms == null ? UNMEASURED : `${Math.round(row.latency_ms)} ms`}</Table.Td>
            </Table.Tr>
          })}</Table.Tbody>
        </Table>
      </TableScrollContainer>
    </>}
  </Card>
}
