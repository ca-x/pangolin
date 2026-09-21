import { useQuery } from '@tanstack/react-query'
import { Activity, Pause, Play } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useLocation, useParams } from 'react-router'
import { api, type Paged, type RequestDetail, type RequestItem } from '../api'
import { EmptyState, SkeletonRows, Status } from '../components'
import { projectOperationPath, useProject } from '../project'
import { PageHeader, QueryError, ResourcePage, displayValue, formatDate } from './shared'
import { Alert, Button, Card, Code, Group, Pagination, SimpleGrid, Stack, Table, TableScrollContainer, Tabs, Text, TextInput, Title } from '@mantine/core'

const TAB_VALUES = ['requests', 'executions', 'threads', 'traces', 'usage', 'costItems', 'audit'] as const

export default function OperationsPage() {
  const { t } = useTranslation()
  const location = useLocation()
  const [tab, setTab] = useState<string>((location.state as { tab?: string } | null)?.tab || 'requests')
  return <Tabs keepMounted={false} value={tab} onChange={(value) => value && setTab(value)} mb="lg">
    <Tabs.List mb="lg">{TAB_VALUES.map(value => <Tabs.Tab key={value} value={value}>{t(value)}</Tabs.Tab>)}</Tabs.List>
    <Tabs.Panel value="requests"><RequestList /></Tabs.Panel>
    <Tabs.Panel value="executions"><ResourcePage resource="executions" title={t('executions')} description={t('executionsDescription')} empty={t('executionEmpty')} immutable columns={[{key:'status',label:t('status')},{key:'request_id',label:t('requestId'),mono:true},{key:'provider_id',label:t('provider'),mono:true},{key:'attempt',label:t('attempt')},{key:'latency_ms',label:t('latency')},{key:'retry_reason',label:t('retryReason')}]} /></Tabs.Panel>
    <Tabs.Panel value="threads"><ResourcePage resource="threads" title={t('threads')} description={t('threadsDescription')} empty={t('threadEmpty')} immutable columns={[{key:'id',label:t('threadId'),mono:true},{key:'external_id',label:t('externalId'),mono:true},{key:'api_key_id',label:t('key'),mono:true},{key:'created_at',label:t('time'),render:formatDate}]} /></Tabs.Panel>
    <Tabs.Panel value="traces"><ResourcePage resource="traces" title={t('traces')} description={t('tracesDescription')} empty={t('traceEmpty')} immutable columns={[{key:'status',label:t('status')},{key:'detail',label:t('trace'),render:(_value,row)=><Button variant="subtle" size="compact-sm" component={Link} to={`/operations/traces/${row.id}`} state={{from:"/operations",tab:"traces"}}>{t('viewTrace')}</Button>},{key:'thread_id',label:t('threadId'),mono:true},{key:'started_at',label:t('startedAt'),render:formatDate},{key:'finished_at',label:t('finishedAt'),render:formatDate}]} /></Tabs.Panel>
    <Tabs.Panel value="usage"><ResourcePage resource="usage" title={t('usage')} description={t('usageDescription')} empty={t('usageEmpty')} immutable columns={[{key:'execution_id',label:t('executionId'),mono:true},{key:'input_tokens',label:t('inputTokens')},{key:'output_tokens',label:t('outputTokens')},{key:'cache_read_tokens',label:t('cacheTokens')},{key:'cost_micros',label:t('cost')},{key:'settlement_kind',label:t('settlement')}]} /></Tabs.Panel>
    <Tabs.Panel value="costItems"><ResourcePage resource="cost-items" title={t('costItems')} description={t('costDescription')} empty={t('costEmpty')} immutable columns={[{key:'usage_id',label:t('usageId'),mono:true},{key:'component_id',label:t('component'),mono:true},{key:'quantity',label:t('quantity')},{key:'subtotal_micros',label:t('cost')}]} /></Tabs.Panel>
    <Tabs.Panel value="audit"><ResourcePage resource="audit" title={t('audit')} description={t('auditDescription')} empty={t('auditEmpty')} immutable columns={[{key:'action',label:t('action')},{key:'resource_type',label:t('type')},{key:'resource_id',label:t('resourceId'),mono:true},{key:'actor_user_id',label:t('actor'),mono:true},{key:'created_at',label:t('time'),render:formatDate}]} /></Tabs.Panel>
  </Tabs>
}

function RequestList() {
  const { t } = useTranslation()
  const { project } = useProject()
  const [paused, setPaused] = useState(false)
  const [offset, setOffset] = useState(0)
  const limit = 25
  const [filters, setFilters] = useState({ status: '', provider: '', model: '' })
  const params = new URLSearchParams({ limit: String(limit), offset: String(offset) })
  const statusCode = filters.status.trim()
  if (statusCode && /^\d{3}$/.test(statusCode)) params.set('status_code', statusCode)
  if (filters.provider) params.set('provider', filters.provider)
  if (filters.model) params.set('model', filters.model)
  const update = (next: typeof filters) => { setFilters(next); setOffset(0) }
  const query = useQuery({
    queryKey: ['requests', project.id, filters, offset],
    queryFn: () => api<Paged<RequestItem>>(`/api/admin/v1/projects/${encodeURIComponent(project.id)}/observability/requests?${params}`),
    refetchInterval: paused ? false : 15000,
  })
  const rows = query.data?.data || []
  const total = query.data?.total || 0
  const totalPages = Math.max(1, Math.ceil(total / limit))
  const currentPage = Math.floor(offset / limit) + 1
  const statusError = filters.status.trim() && !/^\d{3}$/.test(filters.status.trim()) ? t('statusCodeHint') : undefined
  return <>
    <PageHeader title={t('requests')} description={`${t('requestsDescription')} · ${query.dataUpdatedAt ? new Date(query.dataUpdatedAt).toLocaleTimeString() : '—'}`} />
    <Card p="sm" mb="md">
      <Group justify="space-between" align="flex-end" gap="md" wrap="wrap">
        <SimpleGrid cols={{ base: 1, sm: 3 }} style={{ flex: '1 1 480px' }}>
          <TextInput label={t('statusCode')} description={statusError} value={filters.status} onChange={e => update({ ...filters, status: e.target.value })} inputMode="numeric" maxLength={3} placeholder="200" />
          <TextInput label={t('provider')} value={filters.provider} onChange={e => update({ ...filters, provider: e.target.value })} />
          <TextInput label={t('model')} value={filters.model} onChange={e => update({ ...filters, model: e.target.value })} />
        </SimpleGrid>
        <Button onClick={() => setPaused(value => !value)} leftSection={paused ? <Play size={16} /> : <Pause size={16} />}>{paused ? t('resume') : t('pause')}</Button>
      </Group>
    </Card>
    {query.isError ? <QueryError retry={() => void query.refetch()} /> : query.isLoading ? <SkeletonRows /> : !rows.length ? <EmptyState icon={<Activity />} title={t('requests')} copy={t('noRequests24h')} /> : <>
      <TableScrollContainer minWidth={700} style={{ maxHeight: 'min(74vh, 900px)' }}>
        <Table stickyHeader highlightOnHover>
          <Table.Thead><Table.Tr><Table.Th>{t('status')}</Table.Th><Table.Th>{t('model')}</Table.Th><Table.Th>{t('provider')}</Table.Th><Table.Th>{t('endpoint')}</Table.Th><Table.Th>{t('latency')}</Table.Th><Table.Th>{t('cost')}</Table.Th><Table.Th>{t('startedAt')}</Table.Th></Table.Tr></Table.Thead>
          <Table.Tbody>{rows.map(row => <Table.Tr key={row.request_id}><Table.Td><Link to={`/operations/requests/${row.request_id}`}><Status code={row.status_code} /></Link></Table.Td><Table.Td><Text fw={560}>{displayValue(row.requested_model)}</Text></Table.Td><Table.Td>{displayValue(row.provider)}</Table.Td><Table.Td><Code>{row.endpoint}</Code></Table.Td><Table.Td className="mono-cell">{row.latency_ms >= 0 ? `${row.latency_ms} ms` : '—'}</Table.Td><Table.Td className="mono-cell">${(row.cost_micros / 1_000_000).toFixed(6)}</Table.Td><Table.Td>{formatDate(row.started_at)}</Table.Td></Table.Tr>)}</Table.Tbody>
        </Table>
      </TableScrollContainer>
      {total > limit && <Group justify="flex-end" gap="sm" mt="md">
        <Pagination total={totalPages} value={currentPage} onChange={(page) => setOffset((page - 1) * limit)} getControlProps={(control) => {
          if (control === 'previous') return { 'aria-label': t('previous') }
          if (control === 'next') return { 'aria-label': t('next') }
          return {}
        }} />
        <Text size="sm" c="dimmed">{offset + 1}–{Math.min(offset + limit, total)} / {total}</Text>
      </Group>}
    </>}
  </>
}

export function RequestDetailPage() {
  const { id } = useParams()
  const { t } = useTranslation()
  const { project } = useProject()
  const query = useQuery({
    queryKey: ['request', project.id, id],
    queryFn: () => api<RequestDetail>(`/api/admin/v1/projects/${encodeURIComponent(project.id)}/observability/requests/${id}`),
    enabled: Boolean(id),
  })
  return <><PageHeader title={t('requestDetail')} description={id} /><Button variant="subtle" size="compact-sm" component={Link} to="/operations" mb="md">{`← ${t('backToRequests')}`}</Button>
    {query.isError ? <QueryError retry={() => void query.refetch()} /> : query.isLoading ? <SkeletonRows /> : query.data && <TraceFacts data={query.data} />}
  </>
}

type TraceBundle = { trace: { id: string; status: string; started_at: number; finished_at: number | null; thread_id: string | null }; requests: Array<{ id: string; public_id: string; status: string; endpoint: string; model: string; started_at: number; finished_at: number | null }>; executions: Array<{ id: string; request_id: string; status: string; provider_id: string; attempt: number; latency_ms: number | null; retry_reason: string | null; started_at: number; finished_at: number | null }> }

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
        <Card p="md"><Stack gap={4}><Text size="xs" c="dimmed" fw={540}>{t('duration')}</Text><Text fw={620} fz="lg" style={{ fontVariantNumeric: 'tabular-nums' }}>{duration == null ? '—' : `${duration} ms`}</Text></Stack></Card>
        <Card p="md"><Stack gap={4}><Text size="xs" c="dimmed" fw={540}>{t('startedAt')}</Text><Text fw={620} fz="lg" style={{ fontVariantNumeric: 'tabular-nums' }}>{formatDate(data.trace.started_at)}</Text></Stack></Card>
      </SimpleGrid>
      <Stack gap="xs" mb="lg">
        <Title order={2}>{t('requests')}</Title>
        {data.requests.map(request => <Card key={request.id} p="sm">
          <Group justify="space-between" wrap="wrap" mb="xs">
            <Stack gap={2} style={{ minWidth: 0 }}>
              <Text fw={600}>{request.model || request.endpoint}</Text>
              <Text size="sm" c="dimmed">{humanStatus(request.status, t)} · {formatDate(request.started_at)}</Text>
            </Stack>
            <Link to={`/operations/requests/${request.public_id}`} state={{ from: location.pathname }}>{t('viewDetails')}</Link>
          </Group>
          <Code block={false}>{request.id}</Code>
        </Card>)}
      </Stack>
      <Stack gap="xs" mb="lg">
        <Title order={2}>{t('executions')}</Title>
        {data.executions.map(execution => <Card key={execution.id} p="sm">
          <Group justify="space-between" wrap="wrap" mb="xs">
            <Stack gap={2} style={{ minWidth: 0 }}>
              <Text fw={600}>{humanStatus(execution.status, t)}</Text>
              <Text size="sm" c="dimmed">{execution.provider_id || t('unknownProvider')} · {execution.latency_ms == null ? '—' : `${execution.latency_ms} ms`}</Text>
            </Stack>
            <Text size="sm" c="dimmed">{execution.retry_reason ? humanDecision(execution.retry_reason, t) : t('noRetry')}</Text>
          </Group>
          <Code block={false}>{execution.id}</Code>
        </Card>)}
      </Stack>
      <details>
        <Text component="summary" style={{ cursor: 'pointer', padding: '8px 0' }}>{t('rawIdentifiers')}</Text>
        <Code block={false} style={{ display: 'block', marginTop: 8 }}>{data.trace.id}</Code>
        {data.trace.thread_id && <Code block={false} style={{ display: 'block', marginTop: 8 }}>{data.trace.thread_id}</Code>}
      </details>
    </>}
  </>
}

function humanStatus(value: string, t: (key: string) => string) {
  return t(({ succeeded: 'statusSucceeded', finished: 'statusFinished', failed: 'statusFailed', local_failure: 'statusFailed', running: 'statusRunning', pending: 'statusPending', cancelled: 'statusCancelled', interrupted: 'statusInterrupted' } as Record<string, string>)[value] || 'statusUnknown')
}

function humanDecision(value: string, t: (key: string) => string) {
  const key = ({ project_and_model_allowed: 'decisionAllowed', profile_mapping_applied: 'decisionMapped', matched: 'decisionMatched', ordered_candidate: 'decisionOrdered', circuit_open: 'decisionCircuitOpen', disabled: 'decisionDisabled', tag_policy: 'decisionTagPolicy', health_backoff: 'decisionHealthBackoff', quota_exhausted: 'decisionQuotaExhausted', unsupported_endpoint: 'decisionUnsupported' } as Record<string, string>)[value]
  return key ? t(key) : value.replaceAll('_', ' ')
}

function TraceFacts({ data }: { data: RequestDetail }) {
  const { t } = useTranslation()
  const fields: Array<[string, React.ReactNode]> = [
    [t('status'), <Status key="status" code={data.status_code} />],
    [t('trace'), <Code key="trace" block={false}>{data.trace_id}</Code>],
    [t('model'), `${displayValue(data.requested_model)} → ${displayValue(data.resolved_model)}`],
    [t('provider'), displayValue(data.provider)],
    [t('latency'), data.latency_ms >= 0 ? `${data.latency_ms} ms` : '—'],
    ['TTFT', data.ttft_ms == null ? '—' : `${data.ttft_ms} ms`],
    [t('tokens'), `${data.input_tokens} / ${data.output_tokens}`],
    [t('cost'), `$${(data.cost_micros / 1_000_000).toFixed(6)}`],
  ]
  return <Stack gap="lg">
    <Card p={0} style={{ overflow: 'hidden' }}>
      <SimpleGrid cols={{ base: 1, sm: 2 }} spacing={0}>
        {fields.map(([label, value]) => (
          <Stack key={label} gap={4} p="md" style={{ borderBottom: '1px solid var(--mantine-color-default-border)' }}>
            <Text size="xs" c="dimmed" fw={560}>{label}</Text>
            <Text size="sm">{value}</Text>
          </Stack>
        ))}
      </SimpleGrid>
    </Card>
    {data.payload_captured ? <>
      <Json title={t('requestPayload')} value={data.request_json} />
      <Json title={t('responsePayload')} value={data.response_json} />
    </> : <Alert variant="light" color="pangolin" radius="lg">{t('payloadDisabledHint')}</Alert>}
  </Stack>
}

function Json({ title, value }: { title: string; value: string | null }) {
  const { t } = useTranslation()
  const download = () => {
    if (!value) return
    const url = URL.createObjectURL(new Blob([value], { type: 'application/json' }))
    const anchor = document.createElement('a')
    anchor.href = url
    anchor.download = `${title.toLowerCase().replaceAll(' ', '-')}.json`
    anchor.click()
    URL.revokeObjectURL(url)
  }
  return <Card p="md">
    <Group justify="space-between" mb="sm">
      <Title order={3}>{title}</Title>
      {value && <Button variant="subtle" size="compact-sm" onClick={download}>{t('download')}</Button>}
    </Group>
    <Code block style={{ maxHeight: 360, overflow: 'auto' }}>{value ? JSON.stringify(JSON.parse(value), null, 2) : '—'}</Code>
  </Card>
}