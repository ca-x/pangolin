import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Activity, AlertTriangle, Archive, ChevronLeft, ChevronRight, Circle, Copy, Pause, Pin, Play, RotateCcw } from 'lucide-react'
import { useEffect, useRef, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useLocation, useNavigate, useParams, useSearchParams } from 'react-router'
import { toast } from 'sonner'
import { api, type Document, type LiveRequest, type Paged, type RequestDetail, type RequestItem } from '../api'
import { ActionIcon, Alert, Anchor, Badge, Button, Card, Code, Group, MultiSelect, Pagination, Select, SimpleGrid, Stack, Table, TableScrollContainer, Tabs, Text, TextInput, Title, Tooltip } from '@mantine/core'
import { EmptyState, SkeletonRows, Status, confirmAction } from '../components'
import { ObservabilityNotice, UNMEASURED, formatCount, formatMicros, usageMeasured, useErrorKindLabel, useObservability } from '../observability'
import { projectOperationPath, useProject } from '../project'
import { PageHeader, QueryError, ResourcePage, displayValue, formatDate } from './shared'
import { PayloadViewer } from './PayloadViewer'

const TAB_VALUES = ['requests', 'executions', 'threads', 'traces', 'usage', 'costItems', 'audit'] as const

/** Values the current session has actually observed, used to build every facet. */
type Observed = { codes: string[]; providers: string[]; models: string[]; keys: string[] }
const emptyObserved: Observed = { codes: [], providers: [], models: [], keys: [] }

/**
 * The windows the request log offers. Every preset is a closed window resolved at
 * fetch time, so a live-refreshing list keeps sliding with the clock instead of
 * freezing at the moment the page was opened; `custom` reads the two inputs.
 */
const WINDOW_KEYS = [
  { value: '', label: 'anyTime' },
  { value: '1h', label: 'lastHour' },
  { value: '24h', label: 'last24h' },
  { value: '7d', label: 'last7d' },
  { value: '30d', label: 'last30d' },
  { value: 'custom', label: 'customRange' },
] as const
const WINDOW_SECONDS: Record<string, number> = { '1h': 3600, '24h': 86400, '7d': 604800, '30d': 2592000 }

/** A `datetime-local` value is a local wall clock; the API takes unix seconds. */
const toEpoch = (value: string) => {
  const time = new Date(value).getTime()
  return Number.isFinite(time) ? Math.floor(time / 1000) : null
}

function windowBounds(period: string, custom: { from: string; until: string }, now: number) {
  if (period === 'custom') return { from: toEpoch(custom.from), until: toEpoch(custom.until) }
  const span = WINDOW_SECONDS[period]
  return span ? { from: now - span, until: now } : { from: null, until: null }
}

const mergeObserved = (current: Observed, rows: RequestItem[]): Observed => {
  const codes = new Set(current.codes)
  const providers = new Set(current.providers)
  const models = new Set(current.models)
  const keys = new Set(current.keys)
  for (const row of rows) {
    // An unmeasured status is not a code: offering `0` (or `null`) as a filter
    // would promise a bucket the gateway can never emit.
    if (row.status_code != null) codes.add(String(row.status_code))
    if (row.provider) providers.add(row.provider)
    if (row.requested_model) models.add(row.requested_model)
    if (row.resolved_model) models.add(row.resolved_model)
    if (row.api_key_id) keys.add(row.api_key_id)
  }
  const next = { codes: [...codes].sort((a, b) => Number(a) - Number(b)), providers: [...providers].sort(), models: [...models].sort(), keys: [...keys].sort() }
  // Keep the previous object when nothing changed, so the effect cannot loop.
  return next.codes.join() === current.codes.join() && next.providers.join() === current.providers.join() && next.models.join() === current.models.join() && next.keys.join() === current.keys.join() ? current : next
}

const FACET_LIMIT = 20
const facetValues = (params: URLSearchParams, key: string) => params.getAll(key)
  .map((value) => value.trim())
  .filter((value, index, values) => value && values.indexOf(value) === index)
  .slice(0, FACET_LIMIT)
const facetData = (observed: string[], selected: string[]) => [...new Set([...selected, ...observed])]

export default function OperationsPage() {
  const { t } = useTranslation()
  const location = useLocation()
  const observability = useObservability()
  const [tab, setTab] = useState<string>((location.state as { tab?: string } | null)?.tab || 'requests')
  return <><LiveRequestsPanel /><ObservabilityNotice observability={observability} onRetry={observability.refresh} />
    <Tabs keepMounted={false} value={tab} onChange={(value) => value && setTab(value)} mb="lg">
      <Tabs.List mb="lg">{TAB_VALUES.map(value => <Tabs.Tab key={value} value={value}>{t(value)}</Tabs.Tab>)}</Tabs.List>
      <Tabs.Panel value="requests"><RequestList observability={observability} /></Tabs.Panel>
      <Tabs.Panel value="executions"><ResourcePage resource="executions" title={t('executions')} description={t('executionsDescription')} empty={t('executionEmpty')} immutable columns={[{key:'status',label:t('status')},{key:'request_id',label:t('requestId'),mono:true},{key:'provider_id',label:t('provider'),mono:true},{key:'attempt',label:t('attempt')},{key:'latency_ms',label:t('latency')},{key:'retry_reason',label:t('retryReason')}]} /></Tabs.Panel>
      <Tabs.Panel value="threads"><ResourcePage resource="threads" title={t('threads')} description={t('threadsDescription')} empty={t('threadEmpty')} immutable columns={[{key:'id',label:t('threadId'),mono:true},{key:'external_id',label:t('externalId'),mono:true},{key:'api_key_id',label:t('key'),mono:true},{key:'created_at',label:t('time'),render:formatDate}]} /></Tabs.Panel>
      <Tabs.Panel value="traces"><TraceList /></Tabs.Panel>
      <Tabs.Panel value="usage"><ResourcePage resource="usage" title={t('usage')} description={t('usageDescription')} empty={t('usageEmpty')} immutable columns={[{key:'execution_id',label:t('executionId'),mono:true},{key:'input_tokens',label:t('inputTokens')},{key:'output_tokens',label:t('outputTokens')},{key:'cache_read_tokens',label:t('cacheTokens')},{key:'cost_micros',label:t('cost'),render:(value)=>formatMicros(value)},{key:'settlement_kind',label:t('settlement')}]} /></Tabs.Panel>
      <Tabs.Panel value="costItems"><ResourcePage resource="cost-items" title={t('costItems')} description={t('costDescription')} empty={t('costEmpty')} immutable columns={[{key:'usage_id',label:t('usageId'),mono:true},{key:'component_id',label:t('component'),mono:true},{key:'quantity',label:t('quantity')},{key:'subtotal_micros',label:t('cost'),render:(value)=>formatMicros(value)}]} /></Tabs.Panel>
      <Tabs.Panel value="audit"><ResourcePage resource="audit" title={t('audit')} description={t('auditDescription')} empty={t('auditEmpty')} immutable columns={[{key:'action',label:t('action')},{key:'resource_type',label:t('type')},{key:'resource_id',label:t('resourceId'),mono:true},{key:'actor_user_id',label:t('actor'),mono:true},{key:'created_at',label:t('time'),render:formatDate}]} /></Tabs.Panel>
    </Tabs>
  </>
}

function LiveRequestsPanel() {
  const { t } = useTranslation()
  const { project } = useProject()
  const query = useQuery({
    queryKey: ['live-requests', project.id],
    queryFn: () => api<{ enabled: boolean; data: LiveRequest[] }>(`/api/admin/v1/projects/${encodeURIComponent(project.id)}/live-requests`),
    refetchInterval: 3000,
  })
  if (query.isLoading) return <Card p="lg" mb="lg"><SkeletonRows count={2} /></Card>
  if (query.isError) return <Stack mb="lg"><QueryError retry={() => void query.refetch()} /></Stack>
  if (!query.data?.enabled) return null
  const rows = query.data.data
  return <Card p="lg" mb="lg">
    <Stack gap={2} mb="md"><Title order={2}>{t('liveRequests')}</Title><Text size="sm" c="dimmed">{t('liveRequestsHint')}</Text></Stack>
    {!rows.length ? <EmptyState icon={<Activity />} title={t('liveRequests')} copy={t('liveRequestsEmpty')} /> : <TableScrollContainer minWidth={680} role="region" aria-label={t('liveRequests')} tabIndex={0}>
      <Table highlightOnHover>
        <Table.Thead><Table.Tr><Table.Th>{t('model')}</Table.Th><Table.Th>{t('channel')}</Table.Th><Table.Th>{t('key')}</Table.Th><Table.Th>{t('startedAt')}</Table.Th></Table.Tr></Table.Thead>
        <Table.Tbody>{rows.map((row, index) => <Table.Tr key={`${row.api_key_id}-${row.started_at}-${index}`}><Table.Td>{row.model}</Table.Td><Table.Td className="mono-cell">{row.channel_id}</Table.Td><Table.Td className="mono-cell">{row.api_key_id}</Table.Td><Table.Td>{formatDate(row.started_at)}</Table.Td></Table.Tr>)}</Table.Tbody>
      </Table>
    </TableScrollContainer>}
  </Card>
}

/**
 * The trace list's lifecycle facet. `default` is the operator contract's default
 * view — active and retained — and an archived trace is reachable only by asking
 * for it, which is what makes archiving reversible instead of destructive.
 */
type TraceLifecycleView = 'default' | 'active' | 'retained' | 'archived' | 'all'
type TraceLifecycleAction = 'archive' | 'unarchive' | 'retain' | 'unretain'
const TRACE_LIFECYCLE_VIEWS: Array<{ value: TraceLifecycleView; label: string }> = [
  { value: 'default', label: 'traceLifecycleDefault' },
  { value: 'active', label: 'traceLifecycleActive' },
  { value: 'retained', label: 'traceLifecycleRetained' },
  { value: 'archived', label: 'traceLifecycleArchived' },
  { value: 'all', label: 'traceLifecycleAll' },
]
/** The trace's name in a control's accessible name: the client id, else its own. */
const traceName = (row: Document) => String(row.external_id || row.id)

/** The lifecycle as text plus shape plus colour, never a colour alone. */
function TraceLifecycleBadge({ lifecycle }: { lifecycle: string }) {
  const { t } = useTranslation()
  const view = lifecycle === 'archived'
    ? { label: t('traceLifecycleArchived'), color: 'gray', icon: <Archive size={11} strokeWidth={2.8} /> }
    : lifecycle === 'retained'
      ? { label: t('traceLifecycleRetained'), color: 'yellow', icon: <Pin size={11} strokeWidth={2.8} /> }
      : { label: t('traceLifecycleActive'), color: 'teal', icon: <Circle size={11} strokeWidth={3.2} /> }
  return <Badge variant="light" color={view.color} className="status-badge" leftSection={view.icon}>{view.label}</Badge>
}

/**
 * The lifecycle actions one trace row can take, and only those: an active trace is
 * archived or pinned, an archived one is restored, a retained one is released.
 * Archiving is the only action that hides the row, so it asks first and says what
 * it costs; the others are direct.
 *
 * Two facts are captured with the row rather than read from whatever the console
 * shows when a write is answered: the project the row belongs to, which is the one
 * the invalidation names, and the project the console is displaying, which decides
 * whether the success is announced at all. A switch mid-flight therefore refreshes
 * p1 and says nothing on the p2 screen.
 *
 * The refusal is owned by whoever asked: the direct actions report one toast, and
 * archiving reports inside the dialog where the decision was made — a rejected
 * `mutateAsync` surfaces there, so this mutation deliberately has no `onError`.
 */
function TraceLifecycleActions({ row, displayed }: { row: Document; displayed: { current: string } }) {
  const { t } = useTranslation()
  const { project } = useProject()
  const client = useQueryClient()
  const id = String(row.id)
  const name = traceName(row)
  const run = useMutation({
    mutationFn: ({ projectId, action }: { projectId: string; action: TraceLifecycleAction }) => api(`/api/admin/v1/projects/${encodeURIComponent(projectId)}/traces/${encodeURIComponent(id)}/lifecycle`, { method: 'POST', body: JSON.stringify({ action }) }),
    onSuccess: (_result, variables) => {
      void client.invalidateQueries({ queryKey: ['resource', variables.projectId, 'traces'] })
      if (displayed.current === variables.projectId) toast.success(t('saved'))
    },
  })
  const direct = (action: TraceLifecycleAction, label: string, icon: ReactNode) => <Tooltip label={label}><ActionIcon variant="subtle" color="gray" aria-label={`${label} ${name}`} loading={run.isPending} disabled={run.isPending} onClick={() => run.mutate({ projectId: project.id, action }, { onError: (error: Error) => toast.error(error.message) })}>{icon}</ActionIcon></Tooltip>
  const state = String(row.lifecycle ?? 'active')
  if (state === 'archived') return <>{direct('unarchive', t('unarchiveTrace'), <RotateCcw size={16} />)}</>
  if (state === 'retained') return <>{direct('unretain', t('unretainTrace'), <RotateCcw size={16} />)}</>
  return <>
    <Tooltip label={t('archiveTrace')}><ActionIcon variant="subtle" color="gray" aria-label={`${t('archiveTrace')} ${name}`} onClick={() => confirmAction({ title: t('archiveTraceTitle'), body: t('archiveTraceBody'), confirmLabel: t('archiveTrace'), onConfirm: () => run.mutateAsync({ projectId: project.id, action: 'archive' }) })}><Archive size={16} /></ActionIcon></Tooltip>
    {direct('retain', t('retainTrace'), <Pin size={16} />)}
  </>
}

/** The trace tab: the shared resource list plus its lifecycle facet and actions. */
function TraceList() {
  const { t } = useTranslation()
  const { project } = useProject()
  const [view, setView] = useState<TraceLifecycleView>('default')
  // The project on screen, tracked above the rows: a switch replaces every row
  // component, and the mutation a row started keeps the options it was given at
  // click time, so the value it reads at settle time has to live somewhere that
  // survives the switch. Without it a p1 write announced itself on the p2 screen.
  const displayed = useRef(project.id)
  displayed.current = project.id
  // The outcome the run produced, in the console's own language: the stored value
  // is a wire fact, and printing it verbatim put `succeeded` in a translated page.
  const outcome = (value: unknown) => value ? humanStatus(String(value), t) : UNMEASURED
  return <ResourcePage
    resource="traces"
    title={t('traces')}
    description={t('tracesDescription')}
    empty={t('traceEmpty')}
    immutable
    // The view is part of the list's identity, so switching it re-reads the list
    // and the count it reports in the same request.
    endpoint={(projectId) => `${projectOperationPath(projectId, 'traces')}${view === 'default' ? '' : `?lifecycle=${view}`}`}
    filters={<Select aria-label={t('traceLifecycle')} value={view} allowDeselect={false} onChange={(value) => value && setView(value as TraceLifecycleView)} data={TRACE_LIFECYCLE_VIEWS.map((item) => ({ value: item.value, label: t(item.label) }))} w={{ base: '100%', sm: 220 }} />}
    rowActions={(row) => <TraceLifecycleActions row={row} displayed={displayed} />}
    mobileStatus={(row) => outcome(row.status)}
    columns={[
      { key: 'status', label: t('status'), render: outcome },
      { key: 'lifecycle', label: t('traceLifecycle'), render: (value) => <TraceLifecycleBadge lifecycle={String(value ?? 'active')} /> },
      { key: 'detail', label: t('trace'), render: (_value, row) => <Button variant="subtle" size="compact-sm" component={Link} to={`/operations/traces/${row.id}`} state={{ from: '/operations', tab: 'traces' }}>{t('viewTrace')}</Button> },
      { key: 'external_id', label: t('traceExternalId'), mono: true },
      { key: 'request_count', label: t('requestCount') },
      { key: 'first_user_query', label: t('firstUserQuery'), render: (value) => <span title={typeof value === 'string' ? value : undefined}>{displayValue(value)}</span> },
      { key: 'thread_id', label: t('threadId'), mono: true },
      { key: 'started_at', label: t('startedAt'), render: formatDate },
      { key: 'finished_at', label: t('finishedAt'), render: formatDate },
    ]}
  />
}

function RequestList({ observability }: { observability: ReturnType<typeof useObservability> }) {
  const { t } = useTranslation()
  const { project } = useProject()
  const kindLabel = useErrorKindLabel()
  const [paused, setPaused] = useState(false)
  const [searchParams, setSearchParams] = useSearchParams()
  const limit = 25
  const filters = {
    status: facetValues(searchParams, 'status'),
    provider: facetValues(searchParams, 'provider'),
    model: facetValues(searchParams, 'model'),
    key: facetValues(searchParams, 'key'),
  }
  const period = searchParams.get('window') ?? ''
  const custom = { from: searchParams.get('from') ?? '', until: searchParams.get('until') ?? '' }
  const page = Math.max(1, Number.parseInt(searchParams.get('page') ?? '1', 10) || 1)
  const offset = (page - 1) * limit
  const [observed, setObserved] = useState<Observed>(emptyObserved)
  const params = new URLSearchParams({ limit: String(limit), offset: String(offset) })
  const addFacet = (single: string, plural: string, values: string[]) => {
    if (values.length === 1) params.set(single, values[0])
    if (values.length > 1) params.set(plural, JSON.stringify(values))
  }
  addFacet('status_code', 'status_codes', filters.status.filter((value) => /^\d{3}$/.test(value)))
  addFacet('provider', 'providers', filters.provider)
  addFacet('model', 'models', filters.model)
  addFacet('api_key_id', 'api_key_ids', filters.key)
  // The list and its total are read from the same windowed response, so the page
  // count can never describe a different set of rows than the page shows.
  const bounds = windowBounds(period, custom, Math.floor(Date.now() / 1000))
  if (bounds.from != null) params.set('from', String(bounds.from))
  if (bounds.until != null) params.set('until', String(bounds.until))
  const updateParams = (changes: Record<string, string[] | string | null>, replace = false) => {
    const next = new URLSearchParams(searchParams)
    for (const [key, value] of Object.entries(changes)) {
      next.delete(key)
      if (Array.isArray(value)) value.slice(0, FACET_LIMIT).forEach((item) => next.append(key, item))
      else if (value) next.set(key, value)
    }
    if (!Object.hasOwn(changes, 'page')) next.delete('page')
    setSearchParams(next, { replace })
  }
  const query = useQuery({
    queryKey: ['requests', project.id, filters, period, custom, offset],
    queryFn: () => api<Paged<RequestItem>>(`/api/admin/v1/projects/${encodeURIComponent(project.id)}/observability/requests?${params}`),
    refetchInterval: paused || offset > 0 ? false : 15000,
    enabled: observability.measured,
  })
  const rows = query.data?.data || []
  // Facets come from the codes, providers and models this session has actually
  // seen. A hand-typed code could not exist in the projection at all, so the
  // filter would silently promise a status the gateway can never emit.
  useEffect(() => { if (rows.length) setObserved((current) => mergeObserved(current, rows)) }, [query.data])
  const total = query.data?.total || 0
  const totalPages = Math.max(1, Math.ceil(total / limit))
  const currentPage = Math.floor(offset / limit) + 1
  const filtered = Boolean(filters.status.length || filters.provider.length || filters.model.length || filters.key.length || period)
  const showFilters = observability.measured && (filtered || total > 0)
  // Never hide the control that undoes a pause, even once the list drains.
  const showPause = observability.measured && (paused || total > 0)
  const clearFilters = () => updateParams({ status: [], provider: [], model: [], key: [], window: null, from: null, until: null })
  const measured = observability.measured
  const unavailable = observability.reason === 'unavailable'
  return <>
    <PageHeader title={t('requests')} description={`${t('requestsDescription')} · ${query.dataUpdatedAt ? new Date(query.dataUpdatedAt).toLocaleTimeString() : UNMEASURED}`} />
    {(showFilters || showPause) && <Card p="sm" mb="md">
      <Group justify={showFilters ? 'space-between' : 'flex-end'} align="flex-end" gap="md" wrap="wrap">
        {showFilters && <Stack gap="sm" style={{ flex: '1 1 520px' }}>
          <SimpleGrid cols={{ base: 1, sm: 2, lg: 4 }}>
            <MultiSelect label={t('statusCode')} data={facetData(observed.codes, filters.status)} value={filters.status} onChange={(value) => updateParams({ status: value })} placeholder={t('anyStatus')} nothingFoundMessage={t('noObservedValues')} clearable maxValues={FACET_LIMIT} />
            <MultiSelect label={t('provider')} data={facetData(observed.providers, filters.provider)} value={filters.provider} onChange={(value) => updateParams({ provider: value })} placeholder={t('anyProvider')} nothingFoundMessage={t('noObservedValues')} searchable clearable maxValues={FACET_LIMIT} />
            <MultiSelect label={t('model')} data={facetData(observed.models, filters.model)} value={filters.model} onChange={(value) => updateParams({ model: value })} placeholder={t('anyModel')} nothingFoundMessage={t('noObservedValues')} searchable clearable maxValues={FACET_LIMIT} />
            <MultiSelect label={t('keyId')} data={facetData(observed.keys, filters.key)} value={filters.key} onChange={(value) => updateParams({ key: value })} clearable searchable maxValues={FACET_LIMIT} />
            <Select label={t('timeWindow')} data={WINDOW_KEYS.map(({ value, label }) => ({ value, label: t(label) }))} value={period || null} onChange={(value) => updateParams({ window: value ?? null, ...(value === 'custom' ? {} : { from: null, until: null }) })} placeholder={t('anyTime')} clearable />
          </SimpleGrid>
          {/* A custom window is two instants rather than a preset, so it gets its
              own row and only appears once the operator asked for it. */}
          {period === 'custom' && <SimpleGrid cols={{ base: 1, sm: 2 }}>
            <TextInput type="datetime-local" label={t('from')} value={custom.from} onChange={(event) => updateParams({ from: event.target.value || null }, true)} />
            <TextInput type="datetime-local" label={t('until')} value={custom.until} onChange={(event) => updateParams({ until: event.target.value || null }, true)} />
          </SimpleGrid>}
        </Stack>}
        {showPause && <Group gap="xs" align="center">
          {offset > 0 && <Text size="xs" c="dimmed">{t('liveRefreshFirstPage')}</Text>}
          <Button onClick={() => setPaused(value => !value)} leftSection={paused ? <Play size={16} /> : <Pause size={16} />}>{paused ? t('resume') : t('pause')}</Button>
        </Group>}
      </Group>
    </Card>}
    {!measured
      ? <EmptyState icon={<Activity />} title={unavailable ? t('telemetryUnavailableTitle') : t('telemetryNotRecordedTitle')} copy={unavailable ? t('telemetryUnavailableCopy') : t('telemetryNotRecordedCopy')} />
      : query.isError ? <QueryError retry={() => void query.refetch()} /> : query.isLoading ? <SkeletonRows /> : !rows.length ? <EmptyState icon={<Activity />} title={t('requests')} copy={filtered ? t('noSearchResults') : t('noRequests24h')} action={filtered ? <Button variant="default" onClick={clearFilters}>{t('clearFilters')}</Button> : undefined} /> : <>
        <TableScrollContainer minWidth={1500} style={{ maxHeight: 'min(74vh, 900px)' }}>
          <Table stickyHeader highlightOnHover>
            <Table.Thead><Table.Tr><Table.Th>{t('status')}</Table.Th><Table.Th>{t('failureReason')}</Table.Th><Table.Th>{t('requestExternalId')}</Table.Th><Table.Th>{t('model')}</Table.Th><Table.Th>{t('provider')}</Table.Th><Table.Th>{t('endpoint')}</Table.Th><Table.Th>{t('tokens')}</Table.Th><Table.Th>{t('ttft')}</Table.Th><Table.Th>{t('stream')}</Table.Th><Table.Th>{t('latency')}</Table.Th><Table.Th>{t('cost')}</Table.Th><Table.Th>{t('startedAt')}</Table.Th></Table.Tr></Table.Thead>
            {/* The row is keyed and linked by Pangolin's own UUID: the external id is
                chosen by the caller, so two rows can share one and only one of them
                would ever be openable. The external id stays visible for correlation. */}
            <Table.Tbody>{rows.map((row, index) => <Table.Tr key={row.internal_id}><Table.Td><Link to={`/operations/requests/${row.internal_id}`} state={{ from: '/operations', tab: 'requests', ids: rows.map(item => item.internal_id), index }}><Status code={row.status_code} /></Link></Table.Td><Table.Td>{row.error_kind ? kindLabel(row.error_kind) : UNMEASURED}</Table.Td><Table.Td className="mono-cell">{displayValue(row.request_id)}</Table.Td><Table.Td><ModelPair row={row} /></Table.Td><Table.Td>{displayValue(row.provider)}</Table.Td><Table.Td><Code>{row.endpoint}</Code></Table.Td><Table.Td><TokenFacts row={row} /></Table.Td><Table.Td className="mono-cell">{row.ttft_ms == null ? UNMEASURED : `${row.ttft_ms} ms`}</Table.Td><Table.Td>{row.stream == null ? UNMEASURED : t(row.stream ? 'yes' : 'no')}</Table.Td><Table.Td className="mono-cell">{row.latency_ms >= 0 ? `${row.latency_ms} ms` : UNMEASURED}</Table.Td><Table.Td className="mono-cell">{usageMeasured(row) ? formatMicros(row.cost_micros) : UNMEASURED}</Table.Td><Table.Td>{formatDate(row.started_at)}</Table.Td></Table.Tr>)}</Table.Tbody>
          </Table>
        </TableScrollContainer>
        {total > limit && <Group justify="flex-end" gap="sm" mt="md">
          <Pagination total={totalPages} value={currentPage} onChange={(nextPage) => updateParams({ page: nextPage > 1 ? String(nextPage) : null })} getControlProps={(control) => {
            if (control === 'previous') return { 'aria-label': t('previous') }
            if (control === 'next') return { 'aria-label': t('next') }
            return {}
          }} />
          <Text size="sm" c="dimmed">{offset + 1}–{Math.min(offset + limit, total)} / {total}</Text>
        </Group>}
      </>}
  </>
}

/**
 * The requested model is what the client asked for; the resolved model is what the
 * route chose. Both are facts, so both stay on the row: one replacing the other
 * would hide which model was actually called, and `—` is kept for either side
 * independently when that side was not recorded.
 */
function ModelPair({ row }: { row: RequestItem }) {
  return <Group gap={6} wrap="nowrap" style={{ minWidth: 0 }}>
    <Text fw={560} style={{ minWidth: 0 }}>{displayValue(row.requested_model)}</Text>
    <Text c="dimmed" aria-hidden>→</Text>
    <Text style={{ minWidth: 0 }}>{displayValue(row.resolved_model)}</Text>
  </Group>
}

/**
 * The five token facts in one cell, so the log does not grow five narrow columns.
 * The first line is the pair every request has (input / output) under the column's
 * own label; the second names the three facts that only some providers report.
 * A row whose usage was never measured prints one `—` for the whole cell — the
 * same truth rule the cost column uses — while a measured zero stays `0`.
 */
function TokenFacts({ row }: { row: RequestItem }) {
  const { t } = useTranslation()
  if (!usageMeasured(row)) return <>{UNMEASURED}</>
  const facts: Array<[string, number | null | undefined]> = [
    [t('tokenCacheRead'), row.cached_tokens],
    [t('tokenCacheWrite'), row.cache_write_tokens],
    [t('tokenReasoning'), row.reasoning_tokens],
  ]
  return <Stack gap={0}>
    <Text size="sm" className="mono-cell">{`${formatCount(row.input_tokens)} / ${formatCount(row.output_tokens)}`}</Text>
    <Text size="sm" c="dimmed" className="mono-cell">{facts.map(([label, value]) => `${label} ${formatCount(value)}`).join(' · ')}</Text>
  </Stack>
}

/** Navigation context handed over by the request list, so the detail can step through the page it came from. */
type RequestNavigation = { from?: string; tab?: string; ids?: string[]; index?: number }
/** The record-system view of one request: its attempts, usage and cost parts. */
type RequestRecord = {
  executions?: Array<{ id?: string; attempt?: number | null; provider_id?: string | null; provider_name?: string | null; model?: string | null; status?: string | null; latency_ms?: number | null; retry_reason?: string | null; http_status?: number | null; error_kind?: string | null }>
  usage?: Array<{ execution_id?: string; input_tokens?: number; output_tokens?: number; total_cost_micros?: number }>
  cost_items?: Array<{ execution_id?: string; kind?: string | null; quantity?: number | null; unit_price_micros?: number | null; subtotal_micros?: number | null }>
}

export function RequestDetailPage() {
  const { id } = useParams()
  const { t } = useTranslation()
  const { project } = useProject()
  const location = useLocation()
  const navigate = useNavigate()
  const navigation = (location.state as RequestNavigation | null) ?? null
  const query = useQuery({
    queryKey: ['request', project.id, id],
    queryFn: () => api<RequestDetail>(`/api/admin/v1/projects/${encodeURIComponent(project.id)}/observability/requests/${id}`),
    enabled: Boolean(id),
  })
  const data = query.data
  // The card used to tell the operator this endpoint returns no per-attempt list
  // and no per-component costs. It has always returned both; only this read was
  // missing.
  const record = useQuery({
    queryKey: ['request-record', project.id, id],
    queryFn: () => api<RequestRecord>(projectOperationPath(project.id, 'requests', String(id))),
    enabled: Boolean(id),
    retry: false,
  })
  // The detail carries the *external* trace id, while the trace view is keyed by
  // the internal one. The traces resource already exposes both and filters on
  // its document, so the link is resolved from that read instead of guessing.
  const traceLookup = useQuery({
    queryKey: ['trace-for-request', project.id, data?.trace_id],
    queryFn: () => api<Paged<{ id: string; external_id?: string | null }>>(`${projectOperationPath(project.id, 'traces')}?limit=5&q=${encodeURIComponent(String(data?.trace_id))}`),
    enabled: Boolean(data?.trace_id),
    retry: false,
  })
  const internalTraceId = traceLookup.data?.data?.find((row) => row.external_id === data?.trace_id)?.id
  const ids = navigation?.ids ?? []
  const index = typeof navigation?.index === 'number' ? navigation.index : -1
  const step = (target: number) => {
    const next = ids[target]
    if (next) navigate(`/operations/requests/${next}`, { state: { ...navigation, index: target } })
  }
  const backTo = navigation?.from || '/operations'
  return <><PageHeader title={t('requestDetail')} description={id} />
    <Group justify="space-between" align="center" mb="md" wrap="wrap">
      <Button variant="subtle" size="compact-sm" component={Link} to={backTo} state={{ tab: navigation?.tab || 'requests' }}>{`← ${t('backToRequests')}`}</Button>
      {ids.length > 0 && index >= 0 && <Group gap="xs" align="center">
        <Tooltip label={t('previousRequest')}><ActionIcon variant="default" aria-label={t('previousRequest')} disabled={index <= 0} onClick={() => step(index - 1)}><ChevronLeft size={16} /></ActionIcon></Tooltip>
        <Text size="sm" c="dimmed">{t('requestPosition', { index: index + 1, total: ids.length })}</Text>
        <Tooltip label={t('nextRequest')}><ActionIcon variant="default" aria-label={t('nextRequest')} disabled={index < 0 || index >= ids.length - 1} onClick={() => step(index + 1)}><ChevronRight size={16} /></ActionIcon></Tooltip>
      </Group>}
    </Group>
    {query.isError ? <QueryError retry={() => void query.refetch()} /> : query.isLoading ? <SkeletonRows /> : data && <TraceFacts data={data} record={record.data} traceLink={internalTraceId ? `/operations/traces/${internalTraceId}` : undefined} />}
  </>
}

type TraceUsage = { execution_id: string; model_id: string | null; input_tokens: number; output_tokens: number; cache_read_tokens: number; cache_write_tokens: number; reasoning_tokens: number; total_cost_micros: number; created_at: number }
type TraceCostItem = { execution_id: string; kind: string | null; quantity: number; unit_price_micros: number; subtotal_micros: number }
type TraceBundle = { trace: { id: string; status: string; started_at: number; finished_at: number | null; thread_id: string | null }; requests: Array<{ id: string; public_id: string; status: string; endpoint: string; model: string; started_at: number; finished_at: number | null }>; executions: Array<{ id: string; request_id: string; status: string; provider_id: string | null; provider_name?: string | null; model: string | null; attempt: number; latency_ms: number | null; retry_reason: string | null; started_at: number; finished_at: number | null }>; usage?: TraceUsage[]; cost_items?: TraceCostItem[] }

/** The price components a charge can be made of, localized; an unknown kind is shown verbatim. */
const COST_KIND_KEYS: Record<string, string> = { input: 'costKindInput', output: 'costKindOutput', cache_read: 'costKindCacheRead', cache_write: 'costKindCacheWrite', reasoning: 'costKindReasoning', flat: 'costKindFlat', unit: 'costKindUnit' }

export function TraceDetailPage() {
  const { id } = useParams()
  const { t } = useTranslation()
  const { project } = useProject()
  const location = useLocation()
  const query = useQuery({
    queryKey: ['trace', project.id, id],
    queryFn: () => api<TraceBundle>(projectOperationPath(project.id, 'trace-detail', id)),
    enabled: Boolean(id),
  })
  const data = query.data
  const duration = data?.trace.finished_at ? Math.max((data.trace.finished_at - data.trace.started_at) * 1000, ...data.executions.map(item => item.latency_ms || 0)) : null
  const backTo = (location.state as { from?: string } | null)?.from || '/operations'
  const backTab = (location.state as { tab?: string } | null)?.tab || 'traces'
  return <><PageHeader title={t('traceDetail')} description={data ? undefined : id} /><Button variant="subtle" size="compact-sm" component={Link} to={backTo} state={{ tab: backTab }} mb="md">{`← ${t('backToTraces')}`}</Button>
    {query.isError ? <QueryError retry={() => void query.refetch()} /> : query.isLoading ? <SkeletonRows /> : data && <>
      <SimpleGrid cols={{ base: 1, sm: 3 }} mb="lg">
        <Card p="md"><Stack gap={4}><Text size="xs" c="dimmed" fw={540}>{t('outcome')}</Text><Text fw={620} fz="lg" style={{ fontVariantNumeric: 'tabular-nums' }}>{humanStatus(data.trace.status, t)}</Text></Stack></Card>
        <Card p="md"><Stack gap={4}><Text size="xs" c="dimmed" fw={540}>{t('duration')}</Text><Text fw={620} fz="lg" style={{ fontVariantNumeric: 'tabular-nums' }}>{duration == null ? UNMEASURED : `${duration} ms`}</Text></Stack></Card>
        <Card p="md"><Stack gap={4}><Text size="xs" c="dimmed" fw={540}>{t('startedAt')}</Text><Text fw={620} fz="lg" style={{ fontVariantNumeric: 'tabular-nums' }}>{formatDate(data.trace.started_at)}</Text></Stack></Card>
      </SimpleGrid>
      <TraceTimeline trace={data.trace} requests={data.requests} executions={data.executions} from={location.pathname} />
      <TraceAttempts executions={data.executions} usage={data.usage ?? []} costItems={data.cost_items ?? []} />
      <details>
        <Text component="summary" style={{ cursor: 'pointer', padding: '8px 0' }}>{t('rawIdentifiers')}</Text>
        <Code block={false} style={{ display: 'block', marginTop: 8 }}>{data.trace.id}</Code>
        {data.trace.thread_id && <Code block={false} style={{ display: 'block', marginTop: 8 }}>{data.trace.thread_id}</Code>}
      </details>
    </>}
  </>
}

type TimelineRow = { key: string; label: string; status: string; start: number; end: number | null; latencyMs?: number | null; attempt?: number; retryReason?: string | null; externalId?: string; href?: string; nested: boolean }

/**
 * A trace is a span of time, not a list of identifiers: every request and every
 * upstream attempt gets a bar positioned by its real start offset and sized by
 * its real duration, so a retry that took most of the wall clock is visible as
 * one. Each bar keeps its facts as text next to it.
 *
 * A request bar carries two identities and they are not interchangeable: `public_id`
 * is the caller's `x-request-id`, which a caller can repeat inside one project, and
 * `id` is the record's own UUID. The bar opens the request by `id` — the external id
 * cannot say which row it means — and keeps `public_id` on the row so the trace can
 * still be correlated with what the client logged. The bundle coalesces `public_id` to
 * the internal id for a request that carries no external id at all
 * (`src/api/operations_api.rs`), and that fallback is not relabelled as an external id.
 */
function TraceTimeline({ trace, requests, executions, from }: { trace: TraceBundle['trace']; requests: TraceBundle['requests']; executions: TraceBundle['executions']; from: string }) {
  const { t } = useTranslation()
  const start = trace.started_at * 1000
  const rows: TimelineRow[] = [
    ...requests.map((request) => ({ key: `r-${request.id}`, label: request.model || request.endpoint, status: request.status, start: request.started_at * 1000, end: request.finished_at == null ? null : request.finished_at * 1000, externalId: request.public_id === request.id ? undefined : request.public_id, href: `/operations/requests/${request.id}`, nested: false })),
    ...executions.map((execution) => ({ key: `e-${execution.id}`, label: execution.provider_id || t('unknownProvider'), status: execution.status, start: execution.started_at * 1000, end: execution.finished_at == null ? null : execution.finished_at * 1000, latencyMs: execution.latency_ms, attempt: execution.attempt, retryReason: execution.retry_reason, nested: true })),
  ].sort((left, right) => left.start - right.start)
  if (!rows.length) return <EmptyState icon={<Activity />} title={t('executions')} copy={t('traceEmpty')} />
  const last = rows.reduce((value, row) => Math.max(value, row.end ?? row.start), start + 1)
  const span = Math.max(last - start, 1)
  const width = (row: TimelineRow) => Math.max(((row.end ?? last) - row.start) / span * 100, 1.5)
  return <Card component="section" aria-label={t('timeline')} p="lg" mb="lg">
    <Group justify="space-between" align="baseline" mb="md" wrap="wrap">
      <Title order={2}>{t('timeline')}</Title>
      <Text size="sm" c="dimmed">{t('timelineHint')}</Text>
    </Group>
    <Stack gap="xs">
      {rows.map((row) => <div key={row.key} style={{ display: 'grid', gridTemplateColumns: 'minmax(140px, 220px) 1fr', gap: 12, alignItems: 'center', paddingLeft: row.nested ? 18 : 0 }}>
        <Stack gap={2} style={{ minWidth: 0 }}>
          <Group gap={6} wrap="nowrap">
            {row.href ? <Link to={row.href} state={{ from }}>{row.label}</Link> : <Text size="sm" fw={560} truncate>{row.label}</Text>}
            {row.attempt != null && <Text size="xs" c="dimmed">{`#${row.attempt}`}</Text>}
          </Group>
          <Group gap={6} wrap="nowrap">
            <Text size="xs" c="dimmed" truncate>{`${humanStatus(row.status, t)}${row.retryReason ? ` · ${humanDecision(row.retryReason, t)}` : ''}`}</Text>
            {row.externalId && <Text size="xs" c="dimmed" className="mono-cell" aria-label={t('requestExternalId')} truncate>{row.externalId}</Text>}
            {row.href && <Link to={row.href} state={{ from }}><Text size="xs">{t('viewDetails')}</Text></Link>}
          </Group>
        </Stack>
        <div style={{ position: 'relative', height: 26, background: 'var(--mantine-color-default-border)', borderRadius: 4, overflow: 'hidden' }}>
          <div style={{ position: 'absolute', left: `${(row.start - start) / span * 100}%`, width: `${width(row)}%`, top: 5, height: 16, borderRadius: 3, background: row.status === 'succeeded' ? 'var(--accent)' : 'var(--mantine-color-red-5)' }} />
          <Text size="xs" c="dimmed" style={{ position: 'absolute', right: 6, top: 5 }}>{row.latencyMs == null ? UNMEASURED : `${row.latencyMs} ms`}</Text>
        </div>
      </div>)}
    </Stack>
  </Card>
}

/**
 * The attempts of a trace as a list, not only as a picture: which channel each
 * retry used, why the router moved on, what the attempt spent and what it cost.
 * Tokens and money come from `usage_logs`, so an attempt the projection never
 * settled reads `—` rather than a zero nobody measured.
 */
function TraceAttempts({ executions, usage, costItems }: { executions: TraceBundle['executions']; usage: TraceUsage[]; costItems: TraceCostItem[] }) {
  const { t } = useTranslation()
  const totals = usage.reduce((sum, row) => ({
    input: sum.input + row.input_tokens,
    output: sum.output + row.output_tokens,
    cacheRead: sum.cacheRead + row.cache_read_tokens,
    cacheWrite: sum.cacheWrite + row.cache_write_tokens,
    reasoning: sum.reasoning + row.reasoning_tokens,
    cost: sum.cost + row.total_cost_micros,
  }), { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, reasoning: 0, cost: 0 })
  // No usage row at all means the trace was never settled, which is not the same
  // fact as a trace that used nothing.
  const settled = usage.length > 0
  const count = (value: number) => settled ? formatCount(value) : UNMEASURED
  const attemptLabel = (executionId: string) => {
    const attempt = executions.find((item) => item.id === executionId)?.attempt
    return attempt == null ? UNMEASURED : `#${attempt}`
  }
  const costKind = (kind: string | null) => kind == null ? UNMEASURED : COST_KIND_KEYS[kind] ? t(COST_KIND_KEYS[kind]) : kind
  return <>
    <Card p="lg" mb="lg" role="region" aria-label={t('traceUsage')}>
      <Stack gap="md">
        <Group justify="space-between" align="baseline" wrap="wrap">
          <Title order={2}>{t('traceUsage')}</Title>
          <Text size="sm" c="dimmed">{t('traceUsageHint')}</Text>
        </Group>
        <SimpleGrid cols={{ base: 1, xs: 2, sm: 3, lg: 6 }}>
          <Usage label={t('inputTokens')} value={count(totals.input)} />
          <Usage label={t('outputTokens')} value={count(totals.output)} />
          <Usage label={t('cacheReadTokens')} value={count(totals.cacheRead)} />
          <Usage label={t('cacheWriteTokens')} value={count(totals.cacheWrite)} />
          <Usage label={t('reasoningTokens')} value={count(totals.reasoning)} />
          <Usage label={t('cost')} value={settled ? formatMicros(totals.cost) : UNMEASURED} />
        </SimpleGrid>
      </Stack>
    </Card>
    <Card p={0} mb="lg" style={{ overflow: 'hidden' }}>
      <Title order={2} p="lg">{t('attempts')}</Title>
      {!executions.length
        ? <Stack px="lg" pb="lg"><EmptyState icon={<Activity />} title={t('attempts')} copy={t('executionEmpty')} /></Stack>
        : <TableScrollContainer minWidth={1080} role="region" aria-label={t('attempts')} tabIndex={0} style={{ maxHeight: 'min(60vh, 720px)' }}>
        <Table stickyHeader highlightOnHover>
          <Table.Thead><Table.Tr><Table.Th>{t('attempt')}</Table.Th><Table.Th>{t('provider')}</Table.Th><Table.Th>{t('model')}</Table.Th><Table.Th>{t('status')}</Table.Th><Table.Th>{t('retryReason')}</Table.Th><Table.Th>{t('latency')}</Table.Th><Table.Th>{t('inputTokens')}</Table.Th><Table.Th>{t('outputTokens')}</Table.Th><Table.Th>{t('cacheReadTokens')}</Table.Th><Table.Th>{t('cacheWriteTokens')}</Table.Th><Table.Th>{t('reasoningTokens')}</Table.Th><Table.Th>{t('cost')}</Table.Th></Table.Tr></Table.Thead>
          <Table.Tbody>{executions.map((execution) => {
            const row = usage.find((item) => item.execution_id === execution.id)
            return <Table.Tr key={execution.id}>
              <Table.Td className="mono-cell">{`#${execution.attempt}`}</Table.Td>
              {/* The channel the attempt actually ran on: the name frozen when the
                  execution was created, never a live join on the provider. A row
                  from before that snapshot has none, and then its id is the only
                  honest thing left to show. */}
              <Table.Td>{displayValue(execution.provider_name || execution.provider_id)}</Table.Td>
              <Table.Td>{displayValue(execution.model)}</Table.Td>
              <Table.Td>{humanStatus(execution.status, t)}</Table.Td>
              <Table.Td>{execution.retry_reason ? humanDecision(execution.retry_reason, t) : t('noRetry')}</Table.Td>
              <Table.Td className="mono-cell">{execution.latency_ms == null ? UNMEASURED : `${execution.latency_ms} ms`}</Table.Td>
              <Table.Td className="mono-cell">{row ? formatCount(row.input_tokens) : UNMEASURED}</Table.Td>
              <Table.Td className="mono-cell">{row ? formatCount(row.output_tokens) : UNMEASURED}</Table.Td>
              <Table.Td className="mono-cell">{row ? formatCount(row.cache_read_tokens) : UNMEASURED}</Table.Td>
              <Table.Td className="mono-cell">{row ? formatCount(row.cache_write_tokens) : UNMEASURED}</Table.Td>
              <Table.Td className="mono-cell">{row ? formatCount(row.reasoning_tokens) : UNMEASURED}</Table.Td>
              <Table.Td className="mono-cell">{row ? formatMicros(row.total_cost_micros) : UNMEASURED}</Table.Td>
            </Table.Tr>
          })}</Table.Tbody>
        </Table>
      </TableScrollContainer>}
    </Card>
    <Card p={0} mb="lg" style={{ overflow: 'hidden' }}>
      <Title order={2} p="lg">{t('costComponents')}</Title>
      {!costItems.length
        ? <Stack px="lg" pb="lg"><EmptyState icon={<Activity />} title={t('costComponents')} copy={t('costComponentsEmpty')} /></Stack>
        : <TableScrollContainer minWidth={700} role="region" aria-label={t('costComponents')} tabIndex={0}>
          <Table highlightOnHover>
            <Table.Thead><Table.Tr><Table.Th>{t('attempt')}</Table.Th><Table.Th>{t('component')}</Table.Th><Table.Th>{t('quantity')}</Table.Th><Table.Th>{t('unitPrice')}</Table.Th><Table.Th>{t('subtotal')}</Table.Th></Table.Tr></Table.Thead>
            <Table.Tbody>{costItems.map((item, index) => <Table.Tr key={`${item.execution_id}-${index}`}>
              <Table.Td className="mono-cell">{attemptLabel(item.execution_id)}</Table.Td>
              <Table.Td>{costKind(item.kind)}</Table.Td>
              <Table.Td className="mono-cell">{formatCount(item.quantity)}</Table.Td>
              <Table.Td className="mono-cell">{formatMicros(item.unit_price_micros)}</Table.Td>
              <Table.Td className="mono-cell">{formatMicros(item.subtotal_micros)}</Table.Td>
            </Table.Tr>)}</Table.Tbody>
          </Table>
        </TableScrollContainer>}
    </Card>
  </>
}

function humanStatus(value: string, t: (key: string) => string) {
  return t(({ succeeded: 'statusSucceeded', finished: 'statusFinished', failed: 'statusFailed', local_failure: 'statusFailed', running: 'statusRunning', pending: 'statusPending', cancelled: 'statusCancelled', interrupted: 'statusInterrupted' } as Record<string, string>)[value] || 'statusUnknown')
}

function humanDecision(value: string, t: (key: string) => string) {
  const key = ({ project_and_model_allowed: 'decisionAllowed', profile_mapping_applied: 'decisionMapped', matched: 'decisionMatched', ordered_candidate: 'decisionOrdered', circuit_open: 'decisionCircuitOpen', disabled: 'decisionDisabled', tag_policy: 'decisionTagPolicy', health_backoff: 'decisionHealthBackoff', quota_exhausted: 'decisionQuotaExhausted', unsupported_endpoint: 'decisionUnsupported' } as Record<string, string>)[value]
  return key ? t(key) : value.replaceAll('_', ' ')
}

function TraceFacts({ data, record, traceLink }: { data: RequestDetail; record?: RequestRecord; traceLink?: string }) {
  const { t } = useTranslation()
  const kindLabel = useErrorKindLabel()
  // A failure the projection settled as zeros was never measured, so its tokens
  // and cost read `—` instead of an invented `0` next to the alert that says so.
  const measured = usageMeasured(data)
  const count = (value: unknown) => measured ? formatCount(value) : UNMEASURED
  const total = data.input_tokens + data.output_tokens
  const attempts = record?.executions ?? []
  const components = record?.cost_items ?? []
  const costKind = (kind: string | null) => kind == null ? UNMEASURED : COST_KIND_KEYS[kind] ? t(COST_KIND_KEYS[kind]) : kind
  // The upstream status the record system stored for the attempt that failed. The
  // card used to state this was never persisted, which stopped being true when
  // `lifecycle.rs` started writing it — the console was denying data it held.
  const upstreamStatus = attempts.reduce<number | null>((found, attempt) => attempt.http_status == null ? found : attempt.http_status, null)
  const facts: Array<[string, React.ReactNode]> = [
    // The page is opened by Pangolin's own UUID; the client's `x-request-id` is what
    // the operator correlates with, so both are named here instead of one replacing
    // the other.
    [t('requestInternalId'), <Code key="internal-id" block={false}>{data.id}</Code>],
    [t('requestExternalId'), <Code key="external-id" block={false}>{data.request_id}</Code>],
    [t('status'), <Status key="status" code={data.status_code} />],
    [t('endpoint'), <Code key="endpoint" block={false}>{data.endpoint}</Code>],
    [t('trace'), traceLink
      ? <Link key="trace" to={traceLink}>{data.trace_id}</Link>
      : <Group key="trace" gap={4} wrap="nowrap"><Code block={false}>{data.trace_id}</Code><Tooltip label={t('copyTraceId')}><ActionIcon variant="subtle" color="gray" aria-label={t('copyTraceId')} onClick={() => void navigator.clipboard?.writeText(data.trace_id)}><Copy size={15} /></ActionIcon></Tooltip></Group>],
    [t('model'), `${displayValue(data.requested_model)} → ${displayValue(data.resolved_model)}`],
    [t('provider'), displayValue(data.provider)],
    [t('keyId'), displayValue(data.api_key_id)],
    [t('clientIp'), displayValue(data.source_ip)],
    [t('startedAt'), formatDate(data.started_at)],
    [t('finishedAt'), formatDate(data.finished_at)],
    [t('latency'), data.latency_ms >= 0 ? `${data.latency_ms} ms` : UNMEASURED],
    [t('ttft'), data.ttft_ms == null ? UNMEASURED : `${data.ttft_ms} ms`],
    [t('tokens'), `${count(data.input_tokens)} / ${count(data.output_tokens)}`],
    [t('cost'), measured ? formatMicros(data.cost_micros) : UNMEASURED],
  ]
  return <Stack gap="lg">
    {data.error_kind && <Alert variant="light" color="red" radius="lg" icon={<AlertTriangle />} title={t('requestFailedTitle')}>
      <Stack gap={4}>
        <Text size="sm" fw={560}>{kindLabel(data.error_kind)}</Text>
        <Text size="xs" c="dimmed">{upstreamStatus == null ? t('upstreamStatusNotRecorded') : `${t('statusCode')}: ${upstreamStatus}`}</Text>
      </Stack>
    </Alert>}
    <Card p="lg">
      <Stack gap="md">
        <Title order={2}>{t('usageBreakdown')}</Title>
        <SimpleGrid cols={{ base: 1, sm: 3 }}>
          <Usage label={t('inputTokens')} value={count(data.input_tokens)} />
          <Usage label={t('outputTokens')} value={count(data.output_tokens)} />
          <Usage label={t('cacheTokens')} value={count(data.cached_tokens)} />
        </SimpleGrid>
        <Group justify="space-between" align="baseline" wrap="wrap">
          <Text size="sm" c="dimmed">{`${t('tokens')}: ${measured ? formatCount(total) : UNMEASURED}`}</Text>
          <Text size="sm" fw={560}>{`${t('cost')}: ${measured ? formatMicros(data.cost_micros) : UNMEASURED}`}</Text>
        </Group>
        {components.length > 0 ? <Table>
          <Table.Thead><Table.Tr><Table.Th>{t('costComponents')}</Table.Th><Table.Th>{t('quantity')}</Table.Th><Table.Th>{t('cost')}</Table.Th></Table.Tr></Table.Thead>
          <Table.Tbody>{components.map((item, position) => (
            <Table.Tr key={item.execution_id ? `${item.execution_id}-${position}` : position}>
              <Table.Td>{costKind(item.kind ?? null)}</Table.Td>
              <Table.Td>{item.quantity == null ? UNMEASURED : formatCount(item.quantity)}</Table.Td>
              <Table.Td>{item.subtotal_micros == null ? UNMEASURED : formatMicros(item.subtotal_micros)}</Table.Td>
            </Table.Tr>
          ))}</Table.Tbody>
        </Table> : <Text size="xs" c="dimmed">{record ? t('costEmpty') : t('costComponentUnavailable')}</Text>}
        {attempts.length > 0 ? <TableScrollContainer minWidth={620}><Table>
          <Table.Thead><Table.Tr><Table.Th>{t('attempt')}</Table.Th><Table.Th>{t('provider')}</Table.Th><Table.Th>{t('model')}</Table.Th><Table.Th>{t('status')}</Table.Th><Table.Th>{t('latency')}</Table.Th></Table.Tr></Table.Thead>
          <Table.Tbody>{attempts.map((attempt, position) => (
            <Table.Tr key={attempt.id ?? position}>
              <Table.Td>{attempt.attempt == null ? UNMEASURED : `#${attempt.attempt}`}</Table.Td>
              {/* The channel's name as it was when the attempt ran. A rename since
                  then must not relabel this row, and an id is not a name. */}
              <Table.Td>{displayValue(attempt.provider_name)}</Table.Td>
              <Table.Td>{displayValue(attempt.model)}</Table.Td>
              <Table.Td>{displayValue(attempt.status)}</Table.Td>
              <Table.Td>{attempt.latency_ms == null ? UNMEASURED : `${attempt.latency_ms} ms`}</Table.Td>
            </Table.Tr>
          ))}</Table.Tbody>
        </Table></TableScrollContainer> : <Text size="xs" c="dimmed">{record ? `${t('attempts')}: ${UNMEASURED}` : t('attemptsUnavailable')}</Text>}
        {traceLink && <Anchor component={Link} to={traceLink} size="xs">{t('viewTrace')}</Anchor>}
      </Stack>
    </Card>
    <Card p={0} style={{ overflow: 'hidden' }}>
      <SimpleGrid cols={{ base: 1, sm: 2 }} spacing={0}>
        {facts.map(([label, value]) => (
          <Stack key={label} gap={4} p="md" style={{ borderBottom: '1px solid var(--mantine-color-default-border)' }}>
            <Text size="xs" c="dimmed" fw={560}>{label}</Text>
            <Text component="div" size="sm">{value}</Text>
          </Stack>
        ))}
      </SimpleGrid>
    </Card>
    {data.payload_captured
      ? <PayloadViewer requestId={data.id} endpoint={data.endpoint} stream={data.stream} captured requestRaw={data.request_json} responseRaw={data.response_json} />
      : <Alert variant="light" color="pangolin" radius="lg">{t('payloadDisabledHint')}</Alert>}
  </Stack>
}

function Usage({ label, value }: { label: string; value: React.ReactNode }) {
  return <Card p="sm" style={{ border: '1px solid var(--mantine-color-default-border)' }}>
    <Stack gap={2}>
      <Text size="xs" c="dimmed" fw={540}>{label}</Text>
      <Text fw={620} style={{ fontVariantNumeric: 'tabular-nums' }}>{value}</Text>
    </Stack>
  </Card>
}
