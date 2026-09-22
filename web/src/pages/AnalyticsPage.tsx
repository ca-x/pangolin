import { Badge, Card, Group, Select, SimpleGrid, Stack, Table, TableScrollContainer, Text, TextInput, Title } from '@mantine/core'
import { useQuery } from '@tanstack/react-query'
import { BarChart3 } from 'lucide-react'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { useSearchParams } from 'react-router'
import { api, type AnalyticsRow } from '../api'
import { ANALYTICS_DIMENSIONS, type AnalyticsDimension, useAnalyticsDimensionNames } from '../analyticsDimensions'
import { EmptyState, SkeletonRows } from '../components'
import { formatCount, formatMicros, UNMEASURED } from '../observability'
import { useProject } from '../project'
import { PageHeader, QueryError } from './shared'

const RANGES = ['24h', '7d', '30d', 'all', 'custom'] as const
type Range = typeof RANGES[number]
const RANGE_SECONDS: Record<Exclude<Range, 'all' | 'custom'>, number> = { '24h': 86_400, '7d': 604_800, '30d': 2_592_000 }

const epoch = (value: string | null) => {
  const parsed = Number(value)
  return Number.isSafeInteger(parsed) && parsed >= 0 ? parsed : null
}
const localDateTime = (value: number | null) => {
  if (value == null) return ''
  const date = new Date(value * 1000)
  return new Date(date.getTime() - date.getTimezoneOffset() * 60_000).toISOString().slice(0, 16)
}
const fromLocalDateTime = (value: string) => {
  const parsed = new Date(value).getTime()
  return Number.isFinite(parsed) ? Math.floor(parsed / 1000) : null
}

export default function AnalyticsPage() {
  const { t } = useTranslation()
  const { project } = useProject()
  const [params, setParams] = useSearchParams()
  const dimension = ANALYTICS_DIMENSIONS.includes(params.get('dimension') as AnalyticsDimension) ? params.get('dimension') as AnalyticsDimension : 'provider'
  const range = RANGES.includes(params.get('range') as Range) ? params.get('range') as Range : '7d'
  const customFrom = epoch(params.get('from'))
  const customUntil = epoch(params.get('until'))
  const bounds = useMemo(() => {
    const until = range === 'custom' ? customUntil : Math.floor(Date.now() / 1000)
    if (until == null) return null
    if (range === 'custom') return customFrom == null || customFrom >= until ? null : { from: customFrom, until }
    return { from: range === 'all' ? 0 : until - RANGE_SECONDS[range], until }
  }, [range, customFrom, customUntil])
  const update = (key: string, value: string | null) => {
    const next = new URLSearchParams(params)
    if (value) next.set(key, value); else next.delete(key)
    if (key === 'range' && value !== 'custom') { next.delete('from'); next.delete('until') }
    setParams(next, { replace: true })
  }
  const query = useQuery({
    queryKey: ['analytics-page', project.id, dimension, bounds?.from, bounds?.until],
    queryFn: () => api<{ data: AnalyticsRow[] }>(`/api/admin/v1/projects/${encodeURIComponent(project.id)}/analytics?dimension=${dimension}&from=${bounds!.from}&until=${bounds!.until}`),
    enabled: bounds != null,
  })
  const names = useAnalyticsDimensionNames(project.id, dimension)
  const rows = query.data?.data ?? []
  const showConfidence = dimension === 'provider' || dimension === 'model'
  const dimensionOptions = ANALYTICS_DIMENSIONS.map((value) => ({ value, label: t(value === 'api_key' ? 'apiKey' : value) }))
  const rangeOptions = RANGES.map((value) => ({ value, label: t({ '24h': 'last24h', '7d': 'last7d', '30d': 'last30d', all: 'allRetained', custom: 'customRange' }[value]) }))

  return <>
    <PageHeader title={t('analytics')} description={t('analyticsDescription')} />
    <Card p="lg" mb="lg">
      <SimpleGrid cols={{ base: 1, sm: 2 }}>
        <Select label={t('timeWindow')} value={range} data={rangeOptions} onChange={(value) => value && update('range', value)} allowDeselect={false} />
        <Select label={t('dimension')} value={dimension} data={dimensionOptions} onChange={(value) => value && update('dimension', value)} allowDeselect={false} />
      </SimpleGrid>
      {range === 'custom' && <SimpleGrid cols={{ base: 1, sm: 2 }} mt="md">
        <TextInput type="datetime-local" label={t('from')} value={localDateTime(customFrom)} onChange={(event) => update('from', String(fromLocalDateTime(event.currentTarget.value) ?? ''))} />
        <TextInput type="datetime-local" label={t('until')} value={localDateTime(customUntil)} onChange={(event) => update('until', String(fromLocalDateTime(event.currentTarget.value) ?? ''))} />
      </SimpleGrid>}
      {range === 'custom' && !bounds && <Text size="sm" c="red" mt="sm">{t('analyticsRangeInvalid')}</Text>}
    </Card>
    {query.isError ? <QueryError retry={() => void query.refetch()} /> : query.isLoading || names.query.isLoading ? <SkeletonRows count={4} /> : !bounds || !rows.length ? <EmptyState icon={<BarChart3 />} title={t('analytics')} copy={t(!bounds ? 'analyticsRangeInvalid' : 'analyticsEmpty')} /> : <>
      {names.query.isError && <Stack mb="lg"><QueryError retry={() => void names.query.refetch()} /></Stack>}
      <SimpleGrid cols={{ base: 1, md: 2 }} mb="lg">
        <MetricBars title={t('throughput')} rows={rows} label={names.resolve} value={(row) => row.tokens_per_second} format={(value) => `${new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 }).format(value)} ${t('tokensPerSecond')}`} showConfidence={showConfidence} />
        <MetricBars title={t('cost')} rows={rows} label={names.resolve} value={(row) => row.usage_measured === false ? null : row.cost_micros} format={formatMicros} />
      </SimpleGrid>
      <Card p="lg">
        <Group justify="space-between" mb="sm"><Stack gap={2}><Title order={2}>{t('performanceBreakdown')}</Title><Text size="sm" c="dimmed">{t('analyticsMeasuredHint')}</Text></Stack></Group>
        <TableScrollContainer minWidth={showConfidence ? 1080 : 940} scrollAreaProps={{ viewportProps: { role: 'region', 'aria-label': t('performanceBreakdown'), tabIndex: 0 } }}>
          <Table stickyHeader highlightOnHover>
            <Table.Thead><Table.Tr><Table.Th>{t('dimension')}</Table.Th><Table.Th>{t('requests')}</Table.Th><Table.Th>{t('errorsLabel')}</Table.Th><Table.Th>{t('latency')}</Table.Th><Table.Th>{t('ttft')}</Table.Th><Table.Th>{t('throughput')}</Table.Th>{showConfidence && <Table.Th>{t('confidence')}</Table.Th>}<Table.Th>{t('cost')}</Table.Th></Table.Tr></Table.Thead>
            <Table.Tbody>{rows.map((row, index) => {
              const cell = names.resolve(row.dimension)
              return <Table.Tr key={`${row.dimension ?? 'unmeasured'}-${index}`}>
                <Table.Td className={cell.opaque ? 'mono-cell' : undefined}>{cell.text}</Table.Td>
                <Table.Td className="mono-cell">{formatCount(row.requests)}</Table.Td>
                <Table.Td className="mono-cell">{formatCount(row.errors)}</Table.Td>
                <Table.Td className="mono-cell">{row.latency_ms == null ? UNMEASURED : `${Math.round(row.latency_ms)} ms`}</Table.Td>
                <Table.Td className="mono-cell">{row.ttft_ms == null ? UNMEASURED : `${Math.round(row.ttft_ms)} ms`}</Table.Td>
                <Table.Td className="mono-cell">{row.tokens_per_second == null ? UNMEASURED : `${new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 }).format(row.tokens_per_second)} ${t('tokensPerSecond')}`}</Table.Td>
                {showConfidence && <Table.Td><ConfidenceFact row={row} /></Table.Td>}
                <Table.Td className="mono-cell">{row.usage_measured === false ? UNMEASURED : formatMicros(row.cost_micros)}</Table.Td>
              </Table.Tr>
            })}</Table.Tbody>
          </Table>
        </TableScrollContainer>
      </Card>
    </>}
  </>
}

function confidenceLabel(level: AnalyticsRow['confidence_level'], t: (key: string, options?: Record<string, unknown>) => string) {
  return level ? t({ low: 'confidenceLow', medium: 'confidenceMedium', high: 'confidenceHigh' }[level]) : UNMEASURED
}

function ConfidenceFact({ row, compact = false }: { row: AnalyticsRow; compact?: boolean }) {
  const { t } = useTranslation()
  if (typeof row.sample_count !== 'number' || !row.confidence_level) return <>{UNMEASURED}</>
  const sample = t('analyticsSampleCount', { count: row.sample_count })
  if (compact) return <Text size="xs" c={row.confidence_level === 'low' ? 'yellow' : 'dimmed'}>{sample} · {confidenceLabel(row.confidence_level, t)}</Text>
  return <Group gap="xs" wrap="nowrap"><Text size="sm" className="analytics-bar-value">{sample}</Text><Badge variant="light" color={{ low: 'yellow', medium: 'blue', high: 'green' }[row.confidence_level]}>{confidenceLabel(row.confidence_level, t)}</Badge></Group>
}

function MetricBars({ title, rows, label, value, format, showConfidence = false }: { title: string; rows: AnalyticsRow[]; label: (value: string | null) => { text: string; opaque: boolean }; value: (row: AnalyticsRow) => number | null | undefined; format: (value: number) => string; showConfidence?: boolean }) {
  const measured = rows.map(value).filter((item): item is number => typeof item === 'number' && Number.isFinite(item) && item >= 0)
  const max = Math.max(...measured, 0)
  return <Card p="lg" className="analytics-bars" role="img" aria-label={title}>
    <Title order={2} mb="md">{title}</Title>
    <Stack gap="sm">{rows.map((row, index) => {
      const amount = value(row)
      const cell = label(row.dimension)
      const measuredAmount = typeof amount === 'number' && Number.isFinite(amount) && amount >= 0
      return <div className="analytics-bar-row" key={`${row.dimension ?? 'unmeasured'}-${index}`}>
        <Group justify="space-between" gap="md" wrap="nowrap"><Text size="sm" className={cell.opaque ? 'mono-cell' : undefined}>{cell.text}</Text><Stack gap={0} align="flex-end"><Text size="sm" className="analytics-bar-value">{measuredAmount ? format(amount as number) : UNMEASURED}</Text>{showConfidence && <ConfidenceFact row={row} compact />}</Stack></Group>
        <div className="analytics-bar-track" aria-hidden="true"><span data-confidence={showConfidence ? row.confidence_level : undefined} style={{ width: `${measuredAmount && max > 0 ? Math.max(2, (amount as number) / max * 100) : 0}%` }} /></div>
      </div>
    })}</Stack>
  </Card>
}
