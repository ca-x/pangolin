import * as Dialog from '@radix-ui/react-dialog'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Activity, Box, CircleDollarSign, Clock3, KeyRound, Plus, Server, ShieldCheck, Trash2, X } from 'lucide-react'
import { useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { api, type ApiKey, type Model, type Provider, type RequestDetail, type RequestItem, type Summary } from './api'
import { EmptyState, Field, Modal, SelectField, SkeletonRows, Status } from './components'
import { useTheme, type Accent, type ColorMode } from './theme'

function PageHeader({ title, description, action }: { title: string; description?: string; action?: React.ReactNode }) { return <header className="page-header"><div><h1>{title}</h1>{description && <p>{description}</p>}</div>{action}</header> }
function formatNumber(value: number) { return new Intl.NumberFormat(undefined, { notation: value > 9999 ? 'compact' : 'standard', maximumFractionDigits: 1 }).format(value) }
function formatDate(value: number | null) { return value ? new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(value * 1000)) : '—' }
function QueryError({ retry }: { retry: () => void }) { const { t } = useTranslation(); return <div className="query-error" role="alert"><strong>{t('networkError')}</strong><button className="button" onClick={retry}>{t('retry')}</button></div> }
async function copyText(value: string) {
  if (navigator.clipboard?.writeText) { await navigator.clipboard.writeText(value); return }
  const input = document.createElement('textarea'); input.value = value; input.style.position = 'fixed'; input.style.opacity = '0'; document.body.append(input); input.select()
  const copied = document.execCommand('copy'); input.remove(); if (!copied) throw new Error('Clipboard is unavailable')
}

export function Overview() {
  const { t } = useTranslation()
  const summary = useQuery({ queryKey: ['summary'], queryFn: () => api<Summary>('/api/admin/v1/observability/summary') })
  const requests = useQuery({ queryKey: ['requests', 'recent'], queryFn: () => api<RequestItem[]>('/api/admin/v1/observability/requests?limit=6') })
  const value = summary.data
  if (summary.isError || requests.isError) return <><PageHeader title={t('overview')} description={t('last24h')} /><QueryError retry={() => { void summary.refetch(); void requests.refetch() }} /></>
  return <><PageHeader title={t('overview')} description={t('last24h')} />
    <section className="stats-grid" aria-label={t('last24h')}>
      <Stat icon={<Activity />} label={t('totalRequests')} value={formatNumber(value?.requests || 0)} />
      <Stat icon={<ShieldCheck />} label={t('errorRate')} value={`${((value?.error_rate || 0) * 100).toFixed(1)}%`} tone={(value?.error_rate || 0) > .05 ? 'danger' : 'good'} />
      <Stat icon={<Clock3 />} label={t('p95Latency')} value={`${Math.round(value?.p95_latency_ms || 0)} ms`} />
      <Stat icon={<CircleDollarSign />} label={t('cost')} value={`$${((value?.cost_micros || 0) / 1_000_000).toFixed(4)}`} />
    </section>
    <div className="overview-grid"><section className="panel chart-panel"><div className="panel-heading"><div><h2>{t('requestTrend')}</h2><p>{value ? `${formatNumber(value.requests)} · ${formatNumber(value.input_tokens + value.output_tokens)} ${t('tokens')}` : t('loading')}</p></div></div><div className="chart-wrap" aria-label={t('requestTrend')}>{summary.isLoading ? <SkeletonRows count={3} /> : <TrendChart series={value?.series || []} />}</div><details className="chart-table"><summary>{t('dataTable')}</summary><table><tbody>{value?.series.map((point) => <tr key={point.bucket}><td>{formatDate(point.bucket)}</td><td>{point.requests}</td></tr>)}</tbody></table></details></section>
      <section className="panel"><div className="panel-heading"><h2>{t('recentRequests')}</h2></div>{requests.isLoading ? <SkeletonRows /> : <RequestTable rows={requests.data || []} compact />}</section></div>
  </>
}

function Stat({ icon, label, value, tone }: { icon: React.ReactNode; label: string; value: string; tone?: 'good' | 'danger' }) { return <article className={`stat ${tone ? `stat-${tone}` : ''}`}><div className="stat-icon">{icon}</div><div><span>{label}</span><strong>{value}</strong></div></article> }

function TrendChart({ series }: { series: Summary['series'] }) {
  const width = 640; const height = 240; const padding = 18
  if (!series.length) return <div className="chart-empty-line" aria-hidden="true" />
  const max = Math.max(...series.map((point) => point.requests), 1)
  const points = series.map((point, index) => {
    const x = padding + (index / Math.max(series.length - 1, 1)) * (width - padding * 2)
    const y = height - padding - (point.requests / max) * (height - padding * 2)
    return [x, y] as const
  })
  const line = points.map(([x, y], index) => `${index ? 'L' : 'M'} ${x} ${y}`).join(' ')
  const area = `${line} L ${points.at(-1)?.[0] || padding} ${height - padding} L ${points[0]?.[0] || padding} ${height - padding} Z`
  return <svg className="trend-chart" viewBox={`0 0 ${width} ${height}`} preserveAspectRatio="none" aria-hidden="true"><defs><linearGradient id="requestFill" x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stopColor="var(--accent)" stopOpacity=".28"/><stop offset="100%" stopColor="var(--accent)" stopOpacity="0"/></linearGradient></defs>{[.25,.5,.75,1].map((ratio) => <line key={ratio} x1={padding} x2={width-padding} y1={height*ratio-padding/2} y2={height*ratio-padding/2} stroke="var(--border-subtle)" vectorEffect="non-scaling-stroke"/>)}<path d={area} fill="url(#requestFill)"/><path d={line} fill="none" stroke="var(--accent)" strokeWidth="2" vectorEffect="non-scaling-stroke"/></svg>
}

export function Providers() {
  const { t } = useTranslation(); const client = useQueryClient(); const [open, setOpen] = useState(false); const [providerKind, setProviderKind] = useState('openai')
  const query = useQuery({ queryKey: ['providers'], queryFn: () => api<Provider[]>('/api/admin/v1/providers') })
  const create = useMutation({ mutationFn: (body: unknown) => api('/api/admin/v1/providers', { method: 'POST', body: JSON.stringify(body) }), onSuccess: () => { void client.invalidateQueries({ queryKey: ['providers'] }); setOpen(false); toast.success(t('save')) }, onError: (error: Error) => toast.error(error.message) })
  const remove = useMutation({ mutationFn: (id: string) => api(`/api/admin/v1/providers/${id}`, { method: 'DELETE' }), onSuccess: () => void client.invalidateQueries({ queryKey: ['providers'] }) })
  const submit = (event: FormEvent<HTMLFormElement>) => { event.preventDefault(); const data = Object.fromEntries(new FormData(event.currentTarget)); create.mutate({ ...data, kind: providerKind }) }
  return <><PageHeader title={t('providers')} action={<button className="button button-primary" onClick={() => setOpen(true)}><Plus size={17}/>{t('addProvider')}</button>} />
    {query.isError ? <QueryError retry={() => void query.refetch()} /> : query.isLoading ? <SkeletonRows /> : !query.data?.length ? <EmptyState icon={<Server />} title={t('providers')} copy={t('providerEmpty')} action={<button className="button" onClick={() => setOpen(true)}>{t('addProvider')}</button>} /> : <div className="resource-grid">{query.data.map((provider) => <article className="resource-card" key={provider.id}><div className="resource-icon"><Server size={20}/></div><div className="resource-main"><div><h2>{provider.name}</h2><span className="type-label">{provider.kind.replace('_', ' ')}</span></div><code>{provider.base_url}</code></div><button className="icon-button danger" aria-label={t('delete')} onClick={() => confirm(t('deleteConfirm')) && remove.mutate(provider.id)}><Trash2 size={17}/></button></article>)}</div>}
    <Modal open={open} onOpenChange={setOpen} title={t('addProvider')}><form className="form-stack" onSubmit={submit}><Field label={t('providerName')}><input name="name" required autoFocus /></Field><SelectField label={t('providerType')} value={providerKind} onValueChange={setProviderKind} options={[{value:'openai',label:'OpenAI'},{value:'openai_compatible',label:'OpenAI compatible'},{value:'anthropic',label:'Anthropic'}]} /><Field label={t('baseUrl')}><input name="base_url" type="url" defaultValue="https://api.openai.com/v1" required /></Field><Field label={t('upstreamApiKey')}><input name="api_key" type="password" autoComplete="off" required /></Field><div className="form-actions"><button type="button" className="button" onClick={() => setOpen(false)}>{t('cancel')}</button><button className="button button-primary" disabled={create.isPending}>{t('save')}</button></div></form></Modal>
  </>
}

export function Models() {
  const { t } = useTranslation(); const client = useQueryClient(); const [open, setOpen] = useState(false); const [providerId, setProviderId] = useState('')
  const query = useQuery({ queryKey: ['models'], queryFn: () => api<Model[]>('/api/admin/v1/models') }); const providers = useQuery({ queryKey: ['providers'], queryFn: () => api<Provider[]>('/api/admin/v1/providers') })
  const create = useMutation({ mutationFn: (body: unknown) => api('/api/admin/v1/models', { method: 'POST', body: JSON.stringify(body) }), onSuccess: () => { void client.invalidateQueries({ queryKey: ['models'] }); setOpen(false) }, onError: (error: Error) => toast.error(error.message) })
  const remove = useMutation({ mutationFn: (id: string) => api(`/api/admin/v1/models/${id}`, { method: 'DELETE' }), onSuccess: () => void client.invalidateQueries({ queryKey: ['models'] }) })
  const submit = (event: FormEvent<HTMLFormElement>) => { event.preventDefault(); const data = Object.fromEntries(new FormData(event.currentTarget)); create.mutate({ ...data, provider_id: providerId, input_price_micros: Number(data.input_price_micros || 0), output_price_micros: Number(data.output_price_micros || 0), priority: Number(data.priority || 100) }) }
  return <><PageHeader title={t('models')} action={<button className="button button-primary" disabled={!providers.data?.length} onClick={() => { setProviderId(providers.data?.[0]?.id || ''); setOpen(true) }}><Plus size={17}/>{t('addModel')}</button>} />
    {query.isError || providers.isError ? <QueryError retry={() => { void query.refetch(); void providers.refetch() }} /> : query.isLoading ? <SkeletonRows /> : !query.data?.length ? <EmptyState icon={<Box />} title={t('models')} copy={t('modelEmpty')} /> : <div className="table-wrap" role="region" aria-label={t('models')} tabIndex={0}><table><thead><tr><th>{t('publicModel')}</th><th>{t('upstreamModel')}</th><th>{t('provider')}</th><th>{t('priority')}</th><th><span className="sr-only">{t('delete')}</span></th></tr></thead><tbody>{query.data.map((model) => <tr key={model.id}><td><strong>{model.public_name}</strong></td><td><code>{model.upstream_name}</code></td><td>{model.provider_name}</td><td>{model.priority}</td><td className="cell-action"><button className="icon-button danger" aria-label={t('delete')} onClick={() => confirm(t('deleteConfirm')) && remove.mutate(model.id)}><Trash2 size={16}/></button></td></tr>)}</tbody></table></div>}
    <Modal open={open} onOpenChange={setOpen} title={t('addModel')}><form className="form-stack" onSubmit={submit}><SelectField label={t('provider')} value={providerId} onValueChange={setProviderId} options={(providers.data || []).map((provider) => ({ value: provider.id, label: provider.name }))}/><Field label={t('publicModel')}><input name="public_name" required autoFocus placeholder="fast-chat" /></Field><Field label={t('upstreamModel')}><input name="upstream_name" required placeholder="gpt-4.1-mini" /></Field><div className="form-row"><Field label={t('inputPrice')} hint={t('priceHint')}><input name="input_price_micros" type="number" min="0" defaultValue="0" /></Field><Field label={t('outputPrice')} hint={t('priceHint')}><input name="output_price_micros" type="number" min="0" defaultValue="0" /></Field></div><Field label={t('priority')}><input name="priority" type="number" min="0" defaultValue="100" /></Field><div className="form-actions"><button type="button" className="button" onClick={() => setOpen(false)}>{t('cancel')}</button><button className="button button-primary" disabled={create.isPending}>{t('save')}</button></div></form></Modal>
  </>
}

export function Keys() {
  const { t } = useTranslation(); const client = useQueryClient(); const [open, setOpen] = useState(false); const [token, setToken] = useState<string | null>(null)
  const query = useQuery({ queryKey: ['keys'], queryFn: () => api<ApiKey[]>('/api/admin/v1/api-keys') })
  const create = useMutation({ mutationFn: (body: unknown) => api<{ key: ApiKey; token: string }>('/api/admin/v1/api-keys', { method: 'POST', body: JSON.stringify(body) }), onSuccess: (data) => { void client.invalidateQueries({ queryKey: ['keys'] }); setOpen(false); setToken(data.token) }, onError: (error: Error) => toast.error(error.message) })
  const remove = useMutation({ mutationFn: (id: string) => api(`/api/admin/v1/api-keys/${id}`, { method: 'DELETE' }), onSuccess: () => void client.invalidateQueries({ queryKey: ['keys'] }) })
  const submit = (event: FormEvent<HTMLFormElement>) => { event.preventDefault(); const data = Object.fromEntries(new FormData(event.currentTarget)); create.mutate({ name: data.name, budget_micros: data.budget_micros ? Number(data.budget_micros) : null }) }
  return <><PageHeader title={t('apiKeys')} action={<button className="button button-primary" onClick={() => setOpen(true)}><Plus size={17}/>{t('addKey')}</button>} />
    {query.isError ? <QueryError retry={() => void query.refetch()} /> : query.isLoading ? <SkeletonRows /> : !query.data?.length ? <EmptyState icon={<KeyRound />} title={t('apiKeys')} copy={t('keyEmpty')} /> : <div className="table-wrap" role="region" aria-label={t('apiKeys')} tabIndex={0}><table><thead><tr><th>{t('keyName')}</th><th>{t('prefix')}</th><th>{t('budget')}</th><th>{t('lastUsed')}</th><th /></tr></thead><tbody>{query.data.map((key) => <tr key={key.id}><td><strong>{key.name}</strong></td><td><code>pg_{key.key_prefix}_…</code></td><td>{key.budget_micros != null ? `$${(key.budget_micros / 1_000_000).toFixed(2)}` : '∞'}</td><td>{key.last_used_at ? formatDate(key.last_used_at) : t('never')}</td><td className="cell-action"><button className="icon-button danger" aria-label={t('delete')} onClick={() => confirm(t('deleteConfirm')) && remove.mutate(key.id)}><Trash2 size={16}/></button></td></tr>)}</tbody></table></div>}
    <Modal open={open} onOpenChange={setOpen} title={t('addKey')}><form className="form-stack" onSubmit={submit}><Field label={t('keyName')}><input name="name" required autoFocus /></Field><Field label={`${t('budget')} (micro-USD)`}><input name="budget_micros" type="number" min="0" /></Field><div className="form-actions"><button type="button" className="button" onClick={() => setOpen(false)}>{t('cancel')}</button><button className="button button-primary">{t('addKey')}</button></div></form></Modal>
    <Modal open={Boolean(token)} onOpenChange={(value) => !value && setToken(null)} title={t('keyCreated')} description={t('keyCreatedHint')}><div className="secret-reveal"><code>{token}</code><button className="button" onClick={async () => { if (!token) return; try { await copyText(token); toast.success(t('copied')) } catch (error) { toast.error(error instanceof Error ? error.message : t('networkError')) } }}>{t('copy')}</button></div></Modal>
  </>
}

export function Requests() {
  const { t } = useTranslation(); const [selected, setSelected] = useState<string | null>(null)
  const query = useQuery({ queryKey: ['requests'], queryFn: () => api<RequestItem[]>('/api/admin/v1/observability/requests?limit=200'), refetchInterval: 15000 })
  const detail = useQuery({ queryKey: ['request', selected], queryFn: () => api<RequestDetail>(`/api/admin/v1/observability/requests/${selected}`), enabled: Boolean(selected) })
  return <><PageHeader title={t('requests')} description={t('last24h')} />{query.isError ? <QueryError retry={() => void query.refetch()} /> : query.isLoading ? <SkeletonRows /> : <RequestTable rows={query.data || []} onSelect={setSelected} />}
    <Dialog.Root open={Boolean(selected)} onOpenChange={(open) => !open && setSelected(null)}><Dialog.Portal><Dialog.Overlay className="dialog-overlay"/><Dialog.Content className="request-drawer"><div className="dialog-heading"><div><Dialog.Title>{t('requestDetail')}</Dialog.Title><Dialog.Description><code>{selected}</code></Dialog.Description></div><Dialog.Close className="icon-button" aria-label={t('close')}><X size={18}/></Dialog.Close></div>{detail.isError ? <QueryError retry={() => void detail.refetch()} /> : detail.isLoading ? <SkeletonRows /> : detail.data && <RequestDetailView request={detail.data} />}</Dialog.Content></Dialog.Portal></Dialog.Root>
  </>
}

function RequestTable({ rows, compact = false, onSelect }: { rows: RequestItem[]; compact?: boolean; onSelect?: (id: string) => void }) {
  const { t } = useTranslation()
  if (!rows.length) return <EmptyState icon={<Activity />} title={t('requests')} copy={t('noData')} />
  return <div className={`table-wrap ${compact ? 'table-compact' : ''}`} role="region" aria-label={t('requests')} tabIndex={0}><table><thead><tr><th>{t('status')}</th><th>{t('model')}</th><th>{t('provider')}</th>{!compact && <th>{t('endpoint')}</th>}<th>{t('latency')}</th><th>{t('startedAt')}</th></tr></thead><tbody>{rows.map((row) => <tr key={row.request_id} className={onSelect ? 'click-row' : ''} onClick={() => onSelect?.(row.request_id)} tabIndex={onSelect ? 0 : undefined} onKeyDown={(event) => event.key === 'Enter' && onSelect?.(row.request_id)}><td><Status code={row.status_code}/></td><td><strong>{row.requested_model || '—'}</strong></td><td>{row.provider || '—'}</td>{!compact && <td><code>{row.endpoint}</code></td>}<td>{row.latency_ms} ms</td><td>{formatDate(row.started_at)}</td></tr>)}</tbody></table></div>
}

function RequestDetailView({ request }: { request: RequestDetail }) {
  const { t } = useTranslation()
  return <div className="detail-stack"><dl className="detail-grid"><div><dt>{t('status')}</dt><dd><Status code={request.status_code}/></dd></div><div><dt>{t('latency')}</dt><dd>{request.latency_ms} ms</dd></div><div><dt>{t('model')}</dt><dd>{request.requested_model} → {request.resolved_model}</dd></div><div><dt>{t('provider')}</dt><dd>{request.provider}</dd></div><div><dt>{t('tokens')}</dt><dd>{request.input_tokens} in · {request.output_tokens} out</dd></div><div><dt>{t('trace')}</dt><dd><code>{request.trace_id}</code></dd></div></dl>{!request.payload_captured ? <div className="notice"><ShieldCheck size={20}/><div><strong>{t('payloadDisabled')}</strong><p>{t('payloadDisabledHint')}</p></div></div> : <><JsonBlock title={t('requestPayload')} value={request.request_json}/><JsonBlock title={t('responsePayload')} value={request.response_json}/></>}</div>
}
function JsonBlock({ title, value }: { title: string; value: string | null }) { return <section className="json-block"><h3>{title}</h3><pre>{value ? JSON.stringify(JSON.parse(value), null, 2) : '—'}</pre></section> }

export function SettingsPage() {
  const { t, i18n } = useTranslation(); const { mode, accent, setMode, setAccent } = useTheme()
  return <><PageHeader title={t('settings')} /><section className="settings-section"><div><h2>{t('appearance')}</h2><p>{t('productSubtitle')}</p></div><div className="settings-controls"><SelectField label={t('colorMode')} value={mode} onValueChange={(value) => setMode(value as ColorMode)} options={(['system','light','dark'] as const).map((value) => ({ value, label: t(value) }))}/><SelectField label={t('accentTheme')} value={accent} onValueChange={(value) => setAccent(value as Accent)} options={(['bronze','slate','jade'] as const).map((value) => ({ value, label: t(value) }))}/><SelectField label={t('language')} value={i18n.language} onValueChange={(value) => void i18n.changeLanguage(value)} options={[{value:'zh-CN',label:'简体中文'},{value:'en',label:'English'}]}/></div></section><section className="settings-section security-section"><div className="security-icon"><ShieldCheck /></div><div><h2>{t('securityDefaults')}</h2><p>{t('securityCopy')}</p></div></section></>
}
