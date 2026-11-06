import { Card, Stack, Table, TableScrollContainer, Text, Title } from '@mantine/core'
import { useTranslation } from 'react-i18next'
import type { AnalyticsRow } from '../api'
import { formatCount } from '../observability'

type DimensionLabel = (value: string | null) => { text: string; opaque: boolean }
const fields = [
  ['input_tokens', 'inputTokens'],
  ['output_tokens', 'outputTokens'],
  ['total_tokens', 'totalTokens'],
  ['cache_hit_tokens', 'cacheReadTokens'],
  ['cache_write_tokens', 'cacheWriteTokens'],
  ['reasoning_tokens', 'reasoningTokens'],
] as const

export function TokenUsagePanel({ rows, label }: { rows: AnalyticsRow[]; label: DimensionLabel }) {
  const { t } = useTranslation()
  return <Card p="lg" mb="lg">
    <Stack gap={4} mb="md">
      <Title order={2}>{t('tokenUsage')}</Title>
      <Text size="md" c="dimmed">{t('tokenUsageMeasuredHint')}</Text>
    </Stack>
    <TableScrollContainer minWidth={960} scrollAreaProps={{ viewportProps: { role: 'region', 'aria-label': t('tokenUsage'), tabIndex: 0 } }}>
      <Table stickyHeader highlightOnHover>
        <Table.Thead><Table.Tr><Table.Th>{t('dimension')}</Table.Th>{fields.map(([field, name]) => <Table.Th key={field}>{t(name)}</Table.Th>)}</Table.Tr></Table.Thead>
        <Table.Tbody>{rows.map((row, index) => {
          const name = label(row.dimension)
          return <Table.Tr key={`${row.dimension ?? 'unknown'}-${index}`}>
            <Table.Td className={name.opaque ? 'mono-cell' : undefined}>{name.text}</Table.Td>
            {fields.map(([field]) => <Table.Td className="mono-cell" key={field}>
              <Text size="md" fw={field === 'total_tokens' ? 600 : undefined}>{formatCount(row[field] ?? null)}</Text>
              {field === 'total_tokens' && row.total_tokens_samples != null && <Text size="md" c="dimmed">{t('tokenUsageCoverage', { measured: row.total_tokens_samples, total: row.attempts })}</Text>}
            </Table.Td>)}
          </Table.Tr>
        })}</Table.Tbody>
      </Table>
    </TableScrollContainer>
  </Card>
}
