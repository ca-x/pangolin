import { useQuery } from '@tanstack/react-query'
import { Activity, CircleDollarSign, Clock3, Server, ShieldCheck } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router'
import { api, type Paged, type RequestItem, type Summary } from '../api'
import { EmptyState, SkeletonRows } from '../components'
import { PageHeader, QueryError, displayValue, formatDate } from './shared'
import { useProject } from '../project'

const formatNumber = (value: number) => new Intl.NumberFormat(undefined, { notation: value > 9999 ? 'compact' : 'standard', maximumFractionDigits: 1 }).format(value)

export default function OverviewPage() {
  const { t } = useTranslation()
  const { project } = useProject()
  const base = `/api/admin/v1/projects/${encodeURIComponent(project.id)}/observability`
  const summary = useQuery({ queryKey: ['summary', project.id], queryFn: () => api<Summary>(`${base}/summary`) })
  const requests = useQuery({ queryKey: ['requests', project.id, 'recent'], queryFn: () => api<Paged<RequestItem>>(`${base}/requests?limit=6`) })
  const value = summary.data
  if (summary.isError || requests.isError) return <><PageHeader title={t('overview')} description={t('last24h')} /><QueryError retry={() => { void summary.refetch(); void requests.refetch() }} /></>
  return <><PageHeader title={t('overview')} description={t('last24h')} />
    <section className="stats-grid" aria-label={t('last24h')}>
      <Stat icon={<Activity />} label={t('totalRequests')} value={value ? formatNumber(value.requests) : '—'} />
      <Stat icon={<ShieldCheck />} label={t('errorRate')} value={value && value.requests > 0 && value.error_rate != null ? `${(value.error_rate * 100).toFixed(1)}%` : '—'} tone={value?.error_rate != null && value.error_rate > .05 ? 'danger' : undefined} />
      <Stat icon={<Clock3 />} label={t('p95Latency')} value={value && value.requests > 0 && value.p95_latency_ms != null ? `${Math.round(value.p95_latency_ms)} ms` : '—'} />
      <Stat icon={<CircleDollarSign />} label={t('cost')} value={value ? `$${(value.cost_micros / 1_000_000).toFixed(4)}` : '—'} />
    </section>
    {summary.isLoading || requests.isLoading ? <SkeletonRows count={5}/> : value?.requests === 0 ? <section className="overview-empty"><div><Server aria-hidden="true"/><h2>{t('overviewEmptyTitle')}</h2><p>{t('overviewEmptyCopy')}</p></div><Link className="button button-primary" to="/channels">{t('configureChannel')}</Link></section> : <div className="overview-grid"><section className="panel chart-panel"><div className="panel-heading"><div><h2>{t('requestTrend')}</h2><p>{`${formatNumber(value?.requests || 0)} · ${formatNumber((value?.input_tokens || 0) + (value?.output_tokens || 0))} ${t('tokens')}`}</p></div></div><TrendChart series={value?.series || []}/><details className="chart-table"><summary>{t('dataTable')}</summary><table><tbody>{value?.series.map((point) => <tr key={point.bucket}><td>{formatDate(point.bucket)}</td><td>{point.requests}</td></tr>)}</tbody></table></details></section><section className="panel"><div className="panel-heading"><h2>{t('recentRequests')}</h2><Link to="/operations">{t('viewAll')}</Link></div><CompactRequests rows={requests.data?.data || []}/></section></div>}
  </>
}

function Stat({ icon, label, value, tone }: { icon: React.ReactNode; label: string; value: string; tone?: 'danger' }) {
  return <article className={`stat ${tone ? `stat-${tone}` : ''}`}><div className="stat-icon">{icon}</div><div><span>{label}</span><strong>{value}</strong></div></article>
}

function TrendChart({ series }: { series: Summary['series'] }) {
  const width = 640; const height = 240; const padding = 18
  const max = Math.max(...series.map((point) => point.requests), 1)
  const points = series.map((point, index) => ({ x: padding + (index / Math.max(series.length - 1, 1)) * (width - padding * 2), y: height - padding - (point.requests / max) * (height - padding * 2) }))
  const line = points.map((point, index) => `${index ? 'L' : 'M'} ${point.x} ${point.y}`).join(' ')
  return <div className="chart-wrap" aria-label="Request counts by hour"><svg className="trend-chart" viewBox={`0 0 ${width} ${height}`} preserveAspectRatio="none" aria-hidden="true">{[.25,.5,.75,1].map((ratio) => <line key={ratio} x1={padding} x2={width-padding} y1={height*ratio-padding/2} y2={height*ratio-padding/2} stroke="var(--border-subtle)" vectorEffect="non-scaling-stroke"/>)}<path d={line} fill="none" stroke="var(--accent)" strokeWidth="2" vectorEffect="non-scaling-stroke"/>{points.map((point,index)=><circle key={index} cx={point.x} cy={point.y} r="4" fill="var(--accent)" vectorEffect="non-scaling-stroke"/>)}</svg></div>
}

function CompactRequests({ rows }: { rows: RequestItem[] }) {
  const { t } = useTranslation()
  if (!rows.length) return <EmptyState icon={<Activity/>} title={t('recentRequests')} copy={t('noRequests24h')}/>
  return <div className="compact-list">{rows.map((row) => <Link to={`/operations/requests/${row.request_id}`} key={row.request_id}><span className={row.status_code < 400 ? 'status-good' : 'status-bad'}>{row.status_code}</span><strong>{displayValue(row.requested_model)}</strong><small>{row.latency_ms >= 0 ? `${row.latency_ms} ms` : '—'}</small></Link>)}</div>
}
