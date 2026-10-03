import { Text } from '@mantine/core'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { api, type Document } from '../api'
import { componentQuantity, CostFact } from '../measurement'
import { projectOperationPath, useProject } from '../project'
import { QueryError, ResourcePage } from './shared'
/** Read the exact owning usage/price facts, not an unrelated first page of history. */
function ItemFact({ row, quantity = false }: { row: Document; quantity?: boolean }) {
  const { project } = useProject()
  const { t } = useTranslation()
  const usage = useQuery({
    queryKey: ['cost-item-usage', project.id, row.usage_id],
    queryFn: () => api<Document>(projectOperationPath(project.id, 'usage', String(row.usage_id))),
    enabled: typeof row.usage_id === 'string',
    retry: false,
  })
  const price = useQuery({
    queryKey: ['cost-item-price', project.id, usage.data?.price_id],
    queryFn: () => api<Document>(projectOperationPath(project.id, 'prices', String(usage.data?.price_id))),
    enabled: quantity && typeof usage.data?.price_id === 'string',
    retry: false,
  })
  if (usage.isError) return <QueryError retry={() => void usage.refetch()} />
  if (usage.isLoading) return <Text size="sm">{t('loading')}</Text>
  if (!quantity) return <CostFact row={usage.data ?? { pricing_status: 'unavailable' }} value={row.subtotal_micros} />
  if (price.isError) return <QueryError retry={() => void price.refetch()} />
  if (price.isLoading) return <Text size="sm">{t('loading')}</Text>
  const components = Array.isArray(price.data?.components) ? (price.data.components as Document[]) : []
  const kind = components.find((c) => c.id === row.component_id)?.kind
  return <>{componentQuantity(usage.data, kind, row.quantity)}</>
}
export function CostItemsPanel() {
  const { t } = useTranslation()
  return (
    <ResourcePage
      resource="cost-items"
      title={t('costItems')}
      description={t('costDescription')}
      empty={t('costEmpty')}
      immutable
      columns={[
        { key: 'usage_id', label: t('usageId'), mono: true },
        { key: 'component_id', label: t('component'), mono: true },
        { key: 'quantity', label: t('quantity'), render: (_value, row) => <ItemFact row={row} quantity /> },
        { key: 'subtotal_micros', label: t('cost'), render: (_value, row) => <ItemFact row={row} /> },
      ]}
    />
  )
}
