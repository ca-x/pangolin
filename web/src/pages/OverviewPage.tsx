import { useQuery } from '@tanstack/react-query'
import { Activity, CircleDollarSign, Clock3, Server, ShieldCheck } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router'
import { api, type Paged, type RequestItem, type Summary } from '../api'
import { EmptyState, SkeletonRows, Status } from '../components'
import i18n from '../i18n'
import { PageHeader, QueryError, displayValue, formatDate } from './shared'
import { useProject } from '../project'

const formatNumber = (value: number) => new Intl.NumberFormat(i18n.language, { notation: value > 9999 ? 'compact' : 'standard', maximumFractionDigits: 1 }).format(value)
const formatShortDate = (value: number) => new Intl.DateTimeFormat(i18n.language, { month: 'numeric', day: 'numeric', hour: 'numeric' }).format(new Date(value * 1000))
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
  const base = `/api/admin/v1/projects/${encodeURIComponent(project.id)}/observability`
  const summary = useQuery({ queryKey: ['summary', project.id], queryFn: () => api<Summary>(`${base}/summary`) })
  const requests = useQuery({ queryKey: ['requests', project.id, 'recent'], queryFn: () => api<Paged<RequestItem>>(`${base}/requests?limit=6`) })
  const value = summary.data
  if (summary.isError || requests.isError) return <><PageHeader title={t('overview')} description={t('last24h')} /><QueryError retry={() => { void summary.refetch(); void requests.refetch() }} /></>
  const tokens = value ? (value.input_tokens || 0) + (value.output_tokens || 0) : 0
  const failed = value && value.error_rate != null ? Math.round(value.requests * value.error_rate) : null
  return <><PageHeader title={t('overview')} description={t('last24h')} />
    <section className="stats-grid" aria-label={t('last24h')}>
      <Stat icon={<Activity />} label={t('totalRequests')} value={value ? formatNumber(value.requests) : '—'} sub={value && tokens > 0 ? `${formatNumber(tokens)} ${t('tokens')}` : undefined} />
      <Stat icon={<ShieldCheck />} label={t('errorRate')} value={value && value.requests > 0 && value.error_rate != null ? <>{(value.error_rate * 100).toFixed(1)}<em>%</em></> : '—'} sub={failed != null && value!.requests > 0 ? t('failedRequests', { count: failed }) : undefined} tone={value?.error_rate != null && value.error_rate > .05 ? 'danger' : undefined} />
      <Stat icon={<Clock3 />} label={t('p95Latency')} value={value && value.requests > 0 && value.p95_latency_ms != null ? <>{Math.round(value.p95_latency_ms)}<em> ms</em></> : '—'} />
      <Stat icon={<CircleDollarSign />} label={t('cost')} value={value ? <><em>$</em>{(value.cost_micros / 1_000_000).toFixed(4)}</> : '—'} sub={value && value.requests > 0 ? `${(value.cost_micros / value.requests / 1_000_000).toFixed(5)} ${t('perRequest')}` : undefined} />
    </section>
    {summary.isLoading || requests.isLoading ? <SkeletonRows count={5}/> : value?.requests === 0 ? <section className="overview-empty"><div><Server aria-hidden="true"/><h2>{t('overviewEmptyTitle')}</h2><p>{t('overviewEmptyCopy')}</p></div><Link className="button button-primary" to="/channels">{t('configureChannel')}</Link></section> : <div className="overview-grid"><section className="panel chart-panel"><div className="panel-heading"><div><h2>{t('requestTrend')}</h2><p>{`${formatNumber(value?.requests || 0)} · ${formatNumber(tokens)} ${t('tokens')}`}</p></div></div><TrendChart series={value?.series || []}/><details className="chart-table"><summary>{t('dataTable')}</summary><table><tbody>{value?.series.map((point) => <tr key={point.bucket}><td>{formatDate(point.bucket)}</td><td>{point.requests}</td></tr>)}</tbody></table></details></section><section className="panel"><div className="panel-heading"><h2>{t('recentRequests')}</h2><Link to="/operations">{t('viewAll')}</Link></div><CompactRequests rows={requests.data?.data || []}/></section></div>}
  </>
}

function Stat({ icon, label, value, sub, tone }: { icon: React.ReactNode; label: string; value: React.ReactNode; sub?: string; tone?: 'danger' }) {
  return <article className={`stat ${tone ? `stat-${tone}` : ''}`}><div className="stat-icon">{icon}</div><div><span>{label}</span><strong>{value}</strong>{sub && <small className="stat-sub">{sub}</small>}</div></article>
}

type SeriesPoint = Summary['series'][number]

function TrendChart({ series }: { series: SeriesPoint[] }) {
  const { t } = useTranslation()
  const [active, setActive] = useState<number | null>(null)
  if (!series.length) return <div className="chart-empty-line" />
  const width = 640
  const height = 240
  const max = Math.max(...series.map((point) => point.requests), 1)
  const scaleMax = Math.max(1, niceStep((max * 1.08) / 4) * 4)
  const x = (index: number) => (index / Math.max(series.length - 1, 1)) * width
  const y = (count: number) => (1 - count / scaleMax) * height
  const points = series.map((point, index) => ({ x: x(index), y: y(point.requests), ...point }))
  const line = points.map((point, index) => `${index ? 'L' : 'M'} ${point.x.toFixed(2)} ${point.y.toFixed(2)}`).join(' ')
  const area = `${line} L ${width} ${height} L 0 ${height} Z`
  const gridlines = [0, .25, .5, .75, 1]
  const hour = 3600
  const firstBucket = series[0].bucket
  const lastBucket = series[series.length - 1].bucket
  const span = Math.max(lastBucket - firstBucket, 1)
  const stepHours = span >= 18 * hour ? 6 : span >= 9 * hour ? 3 : 1
  const tickTimes: number[] = []
  for (let time = Math.ceil(firstBucket / hour) * hour; time <= lastBucket; time += stepHours * hour) tickTimes.push(time)
  if (!tickTimes.length) tickTimes.push(firstBucket)
  const focus = active == null ? null : points[active]
  const inspect = (event: React.MouseEvent<SVGSVGElement>) => {
    const bounds = event.currentTarget.getBoundingClientRect()
    const ratio = (event.clientX - bounds.left) / bounds.width
    const index = Math.round(ratio * (series.length - 1))
    setActive(Math.min(Math.max(index, 0), series.length - 1))
  }
  return <div className="chart-wrap" aria-label={t('requestTrendHint')}>
    <div className="chart-plot">
      <div className="chart-y-labels" aria-hidden="true">{gridlines.map((ratio) => <span key={ratio} className="chart-axis-label" style={{ top: `${ratio * 100}%` }}>{formatNumber(Math.round(scaleMax * (1 - ratio)))}</span>)}</div>
      <div className="chart-canvas">
        <svg className="trend-chart" viewBox={`0 0 ${width} ${height}`} preserveAspectRatio="none" aria-hidden="true" onMouseMove={inspect} onMouseLeave={() => setActive(null)}>
          <defs><linearGradient id="pangolin-trend-fill" x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stopColor="var(--accent)" stopOpacity=".26"/><stop offset="100%" stopColor="var(--accent)" stopOpacity="0"/></linearGradient></defs>
          {gridlines.map((ratio) => <line key={ratio} className="chart-grid-line" x1="0" x2={width} y1={ratio * height} y2={ratio * height} vectorEffect="non-scaling-stroke"/>)}
          <path className="chart-area" d={area} vectorEffect="non-scaling-stroke"/>
          <path className="chart-line" d={line} vectorEffect="non-scaling-stroke"/>
          {points.length <= 14 && points.map((point) => <circle key={point.bucket} className="chart-dot" cx={point.x} cy={point.y} r="3" vectorEffect="non-scaling-stroke"/>)}
          {focus && <line className="chart-crosshair" x1={focus.x} x2={focus.x} y1="0" y2={height} vectorEffect="non-scaling-stroke"/>}
          {focus && <circle className="chart-dot-active" cx={focus.x} cy={focus.y} r="4.5" vectorEffect="non-scaling-stroke"/>}
        </svg>
        {focus && <div className="chart-tooltip" style={{ left: `${(focus.x / width) * 100}%`, top: `${(focus.y / height) * 100}%` }}><span>{formatShortDate(focus.bucket)}</span><div><strong>{formatNumber(focus.requests)}</strong><span>{t('requests')}</span></div></div>}
      </div>
    </div>
    <div className="chart-x-labels" aria-hidden="true">{tickTimes.map((time) => <span key={time} className="chart-axis-label" style={{ left: `${((time - firstBucket) / span) * 100}%` }}>{formatShortDate(time)}</span>)}</div>
  </div>
}

function CompactRequests({ rows }: { rows: RequestItem[] }) {
  const { t } = useTranslation()
  if (!rows.length) return <EmptyState icon={<Activity/>} title={t('recentRequests')} copy={t('noRequests24h')}/>
  return <div className="compact-list">{rows.map((row) => <Link to={`/operations/requests/${row.request_id}`} key={row.request_id}><Status code={row.status_code} /><strong>{displayValue(row.requested_model)}</strong><small>{row.latency_ms >= 0 ? `${row.latency_ms} ms` : '—'}</small></Link>)}</div>
}
