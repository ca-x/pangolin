import { ActionIcon, Alert, Badge, Button, Checkbox, Collapse, Group, Loader, NumberInput, Pagination, Paper, Select, Stack, Table, TableScrollContainer, Tabs, Text, Textarea, TextInput, Title, Tooltip, UnstyledButton } from '@mantine/core'
import { useMediaQuery } from '@mantine/hooks'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Archive, Copy, Download, Inbox, Plus, RefreshCw, RotateCcw, Search, Trash2, Upload } from 'lucide-react'
import { useEffect, useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { api, type Document, type Paged } from '../api'
import { CapabilityTags, capabilityList, confirmAction, describeFailure, EmptyState, EnabledPill, Modal, SelectField, SkeletonRows, useCapabilityLabel } from '../components'
import { useProject } from '../project'
import { ProviderIcon } from '../ProviderIcon'
import { AutoRefreshControl, useAutoRefreshInterval } from './autoRefresh'
import { PageHeader, QueryError, ResourcePage, displayValue, formatDate } from './shared'

const TAB_VALUES = ['models', 'routing', 'prices', 'catalog', 'subscriptions'] as const

export default function ModelsPage() {
  const { t } = useTranslation()
  return (
    <Tabs keepMounted={false} defaultValue="models">
      <Tabs.List mb="lg">
        {TAB_VALUES.map((value) => (
          <Tabs.Tab key={value} value={value}>{t(value)}</Tabs.Tab>
        ))}
      </Tabs.List>
      <Tabs.Panel value="models"><ModelsPanel /></Tabs.Panel>
      <Tabs.Panel value="routing"><RoutingEditor /><RoutingPreview /></Tabs.Panel>
      <Tabs.Panel value="prices"><PricesPanel /></Tabs.Panel>
      <Tabs.Panel value="catalog"><CatalogPanel /></Tabs.Panel>
      <Tabs.Panel value="subscriptions"><CatalogSubscriptions /></Tabs.Panel>
    </Tabs>
  )
}

function useModelRelations() {
  const { project } = useProject()
  const channels = useQuery({ queryKey: ['channel-options', project.id], queryFn: () => api<Paged<Document>>(`/api/admin/v1/projects/${project.id}/operations/channels?limit=500`) })
  const models = useQuery({ queryKey: ['model-options', project.id], queryFn: () => api<Paged<Document>>(`/api/admin/v1/projects/${project.id}/operations/models?limit=500`) })
  return {
    channels: (channels.data?.data || []).map((row) => ({ value: String(row.id), label: String(row.name) })),
    models: (models.data?.data || []).map((row) => ({ value: String(row.id), label: String(row.public_name) })),
    // A picker fed by a failed lookup would offer an empty choice that reads as a
    // real one, so each select carries the failure and its own retry.
    channelsError: channels.isError,
    modelsError: models.isError,
    retryChannels: () => void channels.refetch(),
    retryModels: () => void models.refetch(),
  }
}

function ModelsPanel() {
  const { t } = useTranslation()
  const { project } = useProject()
  const [lifecycle, setLifecycle] = useState('active')
  const { channels, channelsError, retryChannels } = useModelRelations()
  const catalog = useQuery({ queryKey: ['model-create-catalog', project.id], queryFn: () => api<Paged<Document>>('/api/admin/v1/catalog/models?limit=1000') })
  const catalogOptions = [
    { value: '__manual__', label: t('manualModel') },
    ...(catalog.data?.data || []).map((card) => ({ value: String(card.id), label: String(card.name || card.id) })),
  ]
  const optionsError = channelsError ? t('optionsUnavailable') : undefined
  const modelIdentity = (row: Document) => <span className="provider-cell"><ProviderIcon logoKey={typeof row.catalog_logo_key === 'string' ? row.catalog_logo_key : undefined} name={typeof row.catalog_developer === 'string' ? row.catalog_developer : String(row.public_name)} /><strong>{String(row.public_name)}</strong></span>
  return <>
    <ResourcePage key={project.id} resource="models" endpoint={(projectId) => `/api/admin/v1/projects/${projectId}/operations/models?lifecycle=${lifecycle}`} title={t('models')} description={t('modelsDescription')} selectable={lifecycle === 'active' ? 'models' : undefined} empty={t('modelEmpty')} createLabel={t('addModel')} tableMinWidth={1540} mobileColumnLimit={8} mobilePrimary={modelIdentity} mobilePrimaryKey="public_name" mobileHiddenKeys={['enabled', 'lifecycle']} mobileStatus={(row) => <Stack gap={4} align="flex-end"><LifecyclePill lifecycle={row.lifecycle} /><EnabledPill enabled={row.enabled} /></Stack>} filters={<Select label={t('modelLifecycle')} value={lifecycle} onChange={(value) => setLifecycle(value || 'active')} allowDeselect={false} data={[{ value: 'active', label: t('modelLifecycleActive') }, { value: 'archived', label: t('modelLifecycleArchived') }, { value: 'all', label: t('modelLifecycleAll') }]} w={{ base: '100%', sm: 220 }} />} editDisabled={(row) => row.lifecycle === 'archived'} canDelete={false} rowActions={(row) => <ModelLifecycleActions row={row} />} columns={[{ key: 'public_name', label: t('publicModel'), mono: true, render: (_value, row) => modelIdentity(row) }, { key: 'provider_name', label: t('channel') }, { key: 'upstream_name', label: t('upstreamModel'), mono: true }, { key: 'catalog_developer', label: t('modelDeveloper') }, { key: 'catalog_model_type', label: t('modelType') }, { key: 'catalog_context_limit_tokens', label: t('modelLimits'), render: (_value, row) => <ModelLimits row={row} /> }, { key: 'catalog_input_cost', label: t('catalogCostDefaults'), render: (_value, row) => <CatalogCosts row={row} /> }, { key: 'capabilities', label: t('capabilities'), render: (value) => <CapabilityTags value={value} /> }, { key: 'enabled', label: t('status'), render: (value) => <EnabledPill enabled={value} /> }, { key: 'lifecycle', label: t('modelLifecycle'), render: (value) => <LifecyclePill lifecycle={value} /> }]} fields={[{ key: 'catalog_model_id', label: t('catalogModel'), hint: t('catalogModelHint'), kind: 'select', createOnly: true, defaultValue: '__manual__', options: catalogOptions, loading: catalog.isLoading, error: catalog.isError ? t('catalogModelUnavailable') : undefined, emptyMessage: t('catalogModelEmpty'), allowManualOnError: true, onRetry: () => void catalog.refetch() }, { key: 'provider_id', label: t('channel'), kind: 'select', required: true, options: channels, error: optionsError, onRetry: retryChannels }, { key: 'public_name', label: t('publicModel'), required: true }, { key: 'upstream_name', label: t('upstreamModel'), required: true }, { key: 'capabilities', label: t('capabilities'), kind: 'json', defaultValue: ['chat', 'responses'] }, { key: 'input_price_micros', label: t('inputPrice'), kind: 'number', defaultValue: 0 }, { key: 'output_price_micros', label: t('outputPrice'), kind: 'number', defaultValue: 0 }, { key: 'priority', label: t('priority'), kind: 'number', defaultValue: 100 }, { key: 'enabled', label: t('status'), kind: 'checkbox' }]} normalize={(values, editing) => {
    if (editing || values.catalog_model_id === '__manual__') {
      const manual = { ...values }
      delete manual.catalog_model_id
      return editing ? { ...manual, id: editing.id } : manual
    }
    return values
    }} />
  </>
}

function LifecyclePill({ lifecycle }: { lifecycle: unknown }) {
  const { t } = useTranslation()
  const archived = lifecycle === 'archived'
  return <Badge variant="light" color={archived ? 'gray' : 'green'}>{t(archived ? 'modelLifecycleArchived' : 'modelLifecycleActive')}</Badge>
}

function ModelLifecycleActions({ row }: { row: Document }) {
  const { t } = useTranslation()
  const { project } = useProject()
  const client = useQueryClient()
  const [action, setAction] = useState<'archive' | 'restore' | null>(null)
  const [previewOpen, setPreviewOpen] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const lifecycle = useMutation({
    mutationFn: (action: 'archive' | 'restore') => api(`/api/admin/v1/projects/${project.id}/models/${row.id}/lifecycle`, { method: 'POST', body: JSON.stringify({ action }) }),
    onSuccess: () => { toast.success(t('saved')); setAction(null); void client.invalidateQueries({ queryKey: ['resource', project.id, 'models'] }) },
    onError: (cause: unknown) => setError(describeFailure(cause, t)),
  })
  const name = String(row.public_name || row.id)
  const archived = row.lifecycle === 'archived'
  return <>
    {archived ? <>
      <button type="button" className="model-row-action" aria-label={`${t('restoreModel')} ${name}`} onClick={() => { setError(null); setAction('restore') }}><RotateCcw size={15} />{t('restoreModel')}</button>
      <button type="button" className="model-row-action danger" aria-label={`${t('reviewDeleteImpact')} ${name}`} onClick={() => setPreviewOpen(true)}><Trash2 size={15} />{t('reviewDeleteImpact')}</button>
    </> : <button type="button" className="model-row-action" aria-label={`${t('archiveModel')} ${name}`} onClick={() => { setError(null); setAction('archive') }}><Archive size={15} />{t('archiveModel')}</button>}
    {action && <Modal open onOpenChange={(open) => { if (!open && !lifecycle.isPending) setAction(null) }} title={t(action === 'archive' ? 'archiveModelTitle' : 'restoreModelTitle', { name })} description={t(action === 'archive' ? 'archiveModelBody' : 'restoreModelBody')}>
      <Stack gap="md">
        {error && <Alert color="red" role="alert">{error}</Alert>}
        <Group justify="flex-end" gap="xs"><Button variant="default" disabled={lifecycle.isPending} onClick={() => setAction(null)}>{t('cancel')}</Button><Button color={action === 'archive' ? 'red' : undefined} loading={lifecycle.isPending} onClick={() => lifecycle.mutate(action)}>{t(action === 'archive' ? 'archiveModel' : 'restoreModel')}</Button></Group>
      </Stack>
    </Modal>}
    {previewOpen && <ModelDeletePreview row={row} onClose={() => setPreviewOpen(false)} />}
  </>
}

type ModelDeleteImpact = { prices: number; price_components: number; usage_history: number; associations: number; executions: number; blocked: boolean }

function ModelDeletePreview({ row, onClose }: { row: Document; onClose: () => void }) {
  const { t } = useTranslation()
  const { project } = useProject()
  const client = useQueryClient()
  const [deleteError, setDeleteError] = useState<string | null>(null)
  const name = String(row.public_name || row.id)
  const preview = useQuery({ queryKey: ['model-delete-impact', project.id, row.id], queryFn: () => api<ModelDeleteImpact>(`/api/admin/v1/projects/${project.id}/models/${row.id}/delete-impact`) })
  const remove = useMutation({
    mutationFn: () => api(`/api/admin/v1/projects/${project.id}/operations/models/${row.id}`, { method: 'DELETE' }),
    onSuccess: () => { toast.success(t('deleted')); void client.invalidateQueries({ queryKey: ['resource', project.id, 'models'] }); onClose() },
    onError: (error: unknown) => setDeleteError(describeFailure(error, t)),
  })
  const impact = preview.data
  const total = impact ? impact.prices + impact.price_components + impact.usage_history + impact.associations + impact.executions : 0
  const dependencies = impact ? [
    ['deleteImpactPrices', impact.prices],
    ['deleteImpactPriceComponents', impact.price_components],
    ['deleteImpactUsage', impact.usage_history],
    ['deleteImpactAssociations', impact.associations],
    ['deleteImpactExecutions', impact.executions],
  ] as const : []
  return <Modal open onOpenChange={(open) => { if (!open && !remove.isPending) onClose() }} title={t('deleteModelTitle', { name })} description={t('deleteModelBody')}>
    <Stack gap="md">
      {preview.isLoading ? <Group role="status" gap="xs"><Loader size={16} /><Text size="sm">{t('loading')}</Text></Group> : preview.isError ? <QueryError retry={() => void preview.refetch()} /> : impact && total === 0 ? <Text size="sm" c="dimmed">{t('deleteImpactEmpty')}</Text> : impact ? <Stack component="dl" gap="xs" m={0}>{dependencies.map(([label, count]) => <Group key={label} justify="space-between" wrap="nowrap"><Text component="dt" size="sm" c="dimmed">{t(label)}</Text><Text component="dd" size="sm" fw={600} m={0}>{count}</Text></Group>)}</Stack> : null}
      {impact?.blocked && <Alert color="orange" variant="light">{t('deleteImpactBlocked')}</Alert>}
      {deleteError && <Alert color="red" variant="light" role="alert">{deleteError}</Alert>}
      <Group justify="flex-end" gap="xs">
        <Button variant="default" disabled={remove.isPending} onClick={onClose}>{t('cancel')}</Button>
        <Button color="red" loading={remove.isPending} disabled={!impact || impact.blocked} onClick={() => { setDeleteError(null); remove.mutate() }}>{t('delete')}</Button>
      </Group>
    </Stack>
  </Modal>
}

function formattedLimit(value: unknown, language: string, unit: string) {
  return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0
    ? `${new Intl.NumberFormat(language).format(value)} ${unit}`
    : '—'
}

function ModelLimits({ row }: { row: Document }) {
  const { t, i18n } = useTranslation()
  return <Stack gap={2}><Group gap={6} wrap="nowrap"><Text size="xs" c="dimmed">{t('contextLimit')}</Text><Text size="sm">{formattedLimit(row.catalog_context_limit_tokens, i18n.language, t('tokensUnit'))}</Text></Group><Group gap={6} wrap="nowrap"><Text size="xs" c="dimmed">{t('outputLimit')}</Text><Text size="sm">{formattedLimit(row.catalog_output_limit_tokens, i18n.language, t('tokensUnit'))}</Text></Group></Stack>
}

function formattedCatalogCost(value: unknown, row: Document, language: string, perMillionTokens: string) {
  const currency = typeof row.catalog_cost_currency === 'string' && row.catalog_cost_currency ? row.catalog_cost_currency : null
  const unit = typeof row.catalog_cost_unit === 'string' && row.catalog_cost_unit ? row.catalog_cost_unit : null
  if (typeof value !== 'number' || !Number.isFinite(value) || value < 0 || !currency || !unit) return '—'
  const readableUnit = unit === 'per_million_tokens' ? perMillionTokens : unit.replaceAll('_', ' ')
  return `${new Intl.NumberFormat(language, { maximumFractionDigits: 6 }).format(value)} ${currency} / ${readableUnit}`
}

function CatalogCosts({ row }: { row: Document }) {
  const { t, i18n } = useTranslation()
  return <Stack gap={2}><Group gap={6} wrap="nowrap"><Text size="xs" c="dimmed">{t('catalogInputCost')}</Text><Text size="sm">{formattedCatalogCost(row.catalog_input_cost, row, i18n.language, t('millionTokens'))}</Text></Group><Group gap={6} wrap="nowrap"><Text size="xs" c="dimmed">{t('catalogOutputCost')}</Text><Text size="sm">{formattedCatalogCost(row.catalog_output_cost, row, i18n.language, t('millionTokens'))}</Text></Group></Stack>
}

function RoutingEditor() {
  const { t } = useTranslation()
  const { channels, models, channelsError, modelsError, retryChannels, retryModels } = useModelRelations()
  const any = { value: '__any__', label: t('unrestricted') }
  const matchTypes = [{ value: 'exact', label: t('matchExact') }, { value: 'regex', label: t('matchRegex') }, { value: 'tag', label: t('matchTag') }]
  return <ResourcePage resource="associations" title={t('routing')} description={t('routingDescription')} empty={t('routingEmpty')} createLabel={t('addRoute')} columns={[{ key: 'pattern', label: t('pattern'), mono: true }, { key: 'match_type', label: t('matchType'), render: (value) => matchTypes.find((option) => option.value === value)?.label || displayValue(value) }, { key: 'model_name', label: t('model'), render: (value) => value ? String(value) : t('unrestricted') }, { key: 'provider_name', label: t('channel'), render: (value) => value ? String(value) : t('unrestricted') }, { key: 'priority', label: t('priority') }, { key: 'enabled', label: t('status'), render: (value) => <EnabledPill enabled={value} /> }]} fields={[{ key: 'model_id', label: t('model'), kind: 'select', defaultValue: '__any__', options: [any, ...models], error: modelsError ? t('optionsUnavailable') : undefined, onRetry: retryModels }, { key: 'provider_id', label: t('channel'), kind: 'select', defaultValue: '__any__', options: [any, ...channels], error: channelsError ? t('optionsUnavailable') : undefined, onRetry: retryChannels }, { key: 'match_type', label: t('matchType'), kind: 'select', options: matchTypes }, { key: 'pattern', label: t('pattern'), required: true }, { key: 'conditions', label: t('conditions'), kind: 'json', defaultValue: { version: 1 } }, { key: 'priority', label: t('priority'), kind: 'number', defaultValue: 100 }, { key: 'weight', label: t('weight'), kind: 'number', defaultValue: 1 }, { key: 'enabled', label: t('status'), kind: 'checkbox' }]} normalize={(values, editing) => ({ ...values, ...(editing ? { id: editing.id } : {}), model_id: values.model_id === '__any__' ? null : values.model_id, provider_id: values.provider_id === '__any__' ? null : values.provider_id })} />
}

function PricesPanel() {
  const { t } = useTranslation()
  const { models, modelsError, retryModels } = useModelRelations()
  return <ResourcePage resource="prices" title={t('prices')} description={t('pricesDescription')} empty={t('priceEmpty')} createLabel={t('addPrice')} appendOnly columns={[{ key: 'model_name', label: t('model') }, { key: 'version', label: t('version') }, { key: 'valid_from', label: t('validFrom'), render: formatDate }, { key: 'valid_until', label: t('validUntil'), render: formatDate }, { key: 'schedule', label: t('schedule') }]} fields={[{ key: 'model_id', label: t('model'), kind: 'select', required: true, options: models, error: modelsError ? t('optionsUnavailable') : undefined, onRetry: retryModels }, { key: 'components', label: t('priceComponents'), kind: 'json', required: true, defaultValue: [{ kind: 'input', unit_size: 1000000, unit_price_micros: 0 }] }, { key: 'schedule', label: t('schedule'), kind: 'json', defaultValue: { version: 1, rules: [] } }]} />
}

type Preview = { candidates: Document[]; decisions: Array<{ stage: string; candidate: string | null; reason: string }>; estimated_tokens: number }
const humanRoutingStage = (value: string, t: (key: string) => string) => t(({ access: 'routingStageAccess', mapping: 'routingStageMapping', association: 'routingStageAssociation', eligibility: 'routingStageEligibility', health: 'routingStageHealth', endpoint: 'routingStageEndpoint', strategy: 'routingStageStrategy', protection: 'routingStageProtection' } as Record<string, string>)[value] || 'routingStageOther')
const humanRoutingReason = (value: string, t: (key: string) => string) => {
  const key = ({ project_and_model_allowed: 'decisionAllowed', profile_mapping_applied: 'decisionMapped', matched: 'decisionMatched', ordered_candidate: 'decisionOrdered', circuit_open: 'decisionCircuitOpen', disabled: 'decisionDisabled', tag_policy: 'decisionTagPolicy', health_backoff: 'decisionHealthBackoff', quota_exhausted: 'decisionQuotaExhausted', unsupported_endpoint: 'decisionUnsupported' } as Record<string, string>)[value]
  return key ? t(key) : value.replaceAll('_', ' ')
}

function RoutingPreview() {
  const { t } = useTranslation()
  const { project } = useProject()
  const [result, setResult] = useState<Preview | null>(null)
  const [keyId, setKeyId] = useState('')
  const [endpoint, setEndpoint] = useState('/v1/chat/completions')
  const keys = useQuery({ queryKey: ['keys', project.id], queryFn: () => api<Array<{ id: string; name: string; enabled: boolean }>>(`/api/admin/v1/projects/${project.id}/api-keys`) })
  const preview = useMutation({ mutationFn: (body: unknown) => api<Preview>(`/api/admin/v1/projects/${project.id}/routing-preview`, { method: 'POST', body: JSON.stringify(body) }), onSuccess: setResult, onError: (error: Error) => toast.error(error.message) })
  const submit = (event: FormEvent<HTMLFormElement>) => { event.preventDefault(); const data = new FormData(event.currentTarget); preview.mutate({ api_key_id: keyId, model: data.get('model'), endpoint, body: {} }) }
  const keyOptions = (keys.data || []).filter((key) => key.enabled).map((key) => ({ value: key.id, label: key.name }))
  useEffect(() => { if (!keyOptions.some((option) => option.value === keyId)) setKeyId(keyOptions[0]?.value || '') }, [keyId, keyOptions.map((option) => option.value).join(',')])
  return (
    <Paper component="section" p="lg" withBorder mt="lg">
      <Stack gap="xs" mb="md">
        <Title order={2}>{t('routingPreview')}</Title>
        <Text size="sm" c="dimmed">{t('routingPreviewHint')}</Text>
      </Stack>
      {keys.isError ? <QueryError retry={() => void keys.refetch()} /> : keys.isLoading ? <SkeletonRows count={2} /> : keyOptions.length === 0 ? <Text size="sm" c="dimmed">{t('routingPreviewNeedsKey')}</Text> : (
        <form onSubmit={submit}>
          <Group align="flex-end" gap="sm" wrap="wrap">
            <SelectField name="api_key_id" label={t('apiKey')} value={keyId} onValueChange={setKeyId} options={keyOptions} />
            <TextInput name="model" label={t('requestedModel')} required style={{ minWidth: 160 }} />
            <SelectField name="endpoint" label={t('endpoint')} value={endpoint} onValueChange={setEndpoint} options={[{ value: '/v1/chat/completions', label: t('endpointChatCompletions') }, { value: '/v1/responses', label: t('endpointResponses') }, { value: '/v1/messages', label: t('endpointMessages') }]} />
            <Button type="submit" loading={preview.isPending}>{t('preview')}</Button>
          </Group>
        </form>
      )}
      {result && (
        <Stack gap="md" mt="lg" aria-live="polite">
          <div>
            <Title order={3}>{t('orderedCandidates')}</Title>
            {result.candidates.length
              ? <ol style={{ padding: 0, listStyle: 'none', display: 'grid', gap: 8 }}>{result.candidates.map((row) => <li key={String(row.id)} style={{ display: 'grid', gridTemplateColumns: 'minmax(120px,.5fr) 1fr', gap: 16, padding: '10px 0', borderTop: '1px solid var(--mantine-color-default-border)' }}><Text fw={600}>{displayValue(row.provider)}</Text><Text size="sm" c="dimmed">{displayValue(row.upstream_model)} · {displayValue(row.endpoint)} · {t('weight')} {displayValue(row.weight)}</Text></li>)}</ol>
              : <Text size="sm" c="dimmed">{t('noRouteMatch')}</Text>}
          </div>
          <div>
            <Title order={3}>{t('decisionLog')}</Title>
            <ol style={{ padding: 0, listStyle: 'none', display: 'grid', gap: 8 }}>{result.decisions.map((decision, index) => <li key={`${decision.stage}-${index}`} style={{ display: 'grid', gridTemplateColumns: 'minmax(120px,.5fr) 1fr', gap: 16, padding: '10px 0', borderTop: '1px solid var(--mantine-color-default-border)' }}><Text fw={600}>{humanRoutingStage(decision.stage, t)}</Text><Text size="sm" c="dimmed">{humanRoutingReason(decision.reason, t)}{decision.candidate ? ` · ${decision.candidate}` : ''}</Text></li>)}</ol>
          </div>
        </Stack>
      )}
    </Paper>
  )
}

/**
 * Matches the width where `styles.css` hides `.desktop-resource-table`, so the
 * catalog list swaps at the same point as every other resource page.
 */
const MOBILE_VIEWPORT = '(max-width: 900px)'
/** Outer gap between the catalog tab's groups. Everything inside a group stays
 *  on a ≤16px ladder, so grouping never rests on adjacent borders (audit P2-7). */
const CATALOG_GROUP_GAP = 24
/** Mobile page size. Desktop keeps the shared 25-row default. */
const CATALOG_MOBILE_PAGE_SIZE = 10

/* Capability names as text: at 375px every pill truncated (audit P2-5). The list
   and the label come from the shared helpers, so the catalog reads the same names
   as the models table. */

/**
 * The catalog tab stacks three independent groups — import/export, local
 * overrides and the model list — so it owns the 24px outer gap between them.
 * Below the shared table breakpoint the list is a compact 10-row mobile list
 * instead of `ResourcePage`'s 25-row cards (audit P2-6).
 */
function CatalogPanel() {
  const { t } = useTranslation()
  const mobile = useMediaQuery(MOBILE_VIEWPORT, undefined, { getInitialValueInEffect: false })
  return (
    <Stack gap={CATALOG_GROUP_GAP}>
      <CatalogMaintenance />
      <CatalogOverrideEditor />
      {mobile
        ? <CatalogModelsMobile />
        : <ResourcePage resource="catalog-models" endpoint="/api/admin/v1/catalog/models" title={t('catalog')} description={t('catalogDescription')} empty={t('catalogEmpty')} immutable columns={[{ key: 'id', label: t('modelId'), mono: true }, { key: 'name', label: t('name'), mono: true }, { key: 'type', label: t('type') }, { key: 'capabilities', label: t('capabilities'), render: (value) => <CapabilityTags value={value} /> }, { key: 'provider_id', label: t('provider') }]} />}
    </Stack>
  )
}

/**
 * Mobile catalog list. Title/description/search/result count sit on an
 * 8/12/16px ladder (the shared `h1` rule adds 6px under the title, so the
 * rendered title-to-description gap is 14px), each card is a two-line summary,
 * and the result count is repeated above the list so the page size is knowable
 * without scrolling.
 */
function CatalogModelsMobile() {
  const { t } = useTranslation()
  const { project } = useProject()
  const [offset, setOffset] = useState(0)
  const [filter, setFilter] = useState('')
  const [pageSize, setPageSize] = useState(CATALOG_MOBILE_PAGE_SIZE)
  const path = `/api/admin/v1/catalog/models?offset=${offset}&limit=${pageSize}${filter.trim() ? `&q=${encodeURIComponent(filter.trim())}` : ''}`
  const query = useQuery({ queryKey: ['resource', project.id, 'catalog-models', path], queryFn: () => api<Paged<Document>>(path) })
  const rows = query.data?.data || []
  const total = query.data?.total ?? rows.length
  const totalPages = Math.max(1, Math.ceil(total / pageSize))
  const currentPage = Math.floor(offset / pageSize) + 1
  const changePageSize = (value: string | null) => { if (value) { setPageSize(Number(value)); setOffset(0) } }
  return (
    <div>
      <Stack gap={8}>
        <Title order={1}>{t('catalog')}</Title>
        <Text size="md" c="dimmed">{t('catalogDescription')}</Text>
      </Stack>
      {(total > 0 || filter) && <TextInput leftSection={<Search size={17} />} placeholder={t('search')} aria-label={t('search')} value={filter} onChange={(event) => { setFilter(event.currentTarget.value); setOffset(0) }} w={{ base: '100%', sm: 360 }} style={{ marginTop: 12 }} />}
      {query.isError ? <QueryError retry={() => void query.refetch()} /> : query.isLoading ? <SkeletonRows /> : rows.length === 0 ? <EmptyState icon={<Inbox />} title={t('catalog')} copy={filter ? t('noSearchResults') : t('catalogEmpty')} /> : (
        <>
          <Text size="md" c="dimmed" style={{ marginBottom: 16 }}>{t('catalogResultCount', { count: total })}</Text>
          <Stack gap={8}>{rows.map((row) => <CatalogModelCard key={row.id} row={row} />)}</Stack>
        </>
      )}
      {total > pageSize && (
        <Group justify="flex-end" gap="sm" mt={16} className="pagination">
          <Pagination total={totalPages} value={currentPage} onChange={(page) => setOffset((page - 1) * pageSize)} getControlProps={(control) => {
            if (control === 'previous') return { 'aria-label': t('previous') }
            if (control === 'next') return { 'aria-label': t('next') }
            return {}
          }} />
          <Text size="sm" c="dimmed">{offset + 1}–{Math.min(offset + pageSize, total)} / {total}</Text>
          <Select aria-label={t('pageSize')} data={['10', '25', '50']} value={String(pageSize)} onChange={changePageSize} size="sm" style={{ width: 80 }} />
        </Group>
      )}
    </div>
  )
}

/** One catalog model as a two-line summary: name, then type and key capabilities. */
function CatalogModelCard({ row }: { row: Document }) {
  const { t } = useTranslation()
  const name = displayValue(row.name)
  const label = useCapabilityLabel()
  const names = capabilityList(row.capabilities).map(label)
  const shown = names.slice(0, 2)
  const extra = names.length - shown.length
  const summary = [displayValue(row.type), ...shown, ...(extra > 0 ? [t('moreCapabilities', { count: extra })] : [])].join(' · ')
  return (
    <Paper p="md" withBorder>
      <Group justify="space-between" align="flex-start" gap="xs" wrap="nowrap">
        <Stack gap={4} style={{ flex: 1, minWidth: 0 }}>
          <Text fw={600} size="md" style={{ overflowWrap: 'anywhere' }}>{name}</Text>
          <Text size="md" c="dimmed" style={{ overflowWrap: 'anywhere' }}>{summary}</Text>
        </Stack>
        <ActionIcon variant="subtle" color="gray" aria-label={`${t('copyId')} ${name}`} onClick={() => void navigator.clipboard?.writeText(String(row.id))}><Copy size={16} /></ActionIcon>
      </Group>
    </Paper>
  )
}

function CatalogMaintenance() {
  const { t } = useTranslation()
  const client = useQueryClient()
  const [importOpen, setImportOpen] = useState(false)
  const importCatalog = useMutation({ mutationFn: (document: unknown) => api('/api/admin/v1/catalog/import', { method: 'POST', body: JSON.stringify(document) }), onSuccess: () => { toast.success(t('saved')); void client.invalidateQueries({ queryKey: ['resource'] }); setImportOpen(false) }, onError: (error: Error) => toast.error(error.message) })
  const submit = (event: FormEvent<HTMLFormElement>) => { event.preventDefault(); const raw = String(new FormData(event.currentTarget).get('catalog') || ''); try { importCatalog.mutate(JSON.parse(raw)) } catch { toast.error(t('invalidJson')) } }
  return (
    <Paper p="lg" withBorder style={{ position: 'relative' }}>
      <Group justify="space-between" align="flex-start" wrap="nowrap">
        <Stack gap="xs">
          <Title order={2}>{t('catalogTransfer')}</Title>
          <Text size="sm" c="dimmed">{t('catalogTransferHint')}</Text>
        </Stack>
        <Group>
          <Button component="a" href="/api/admin/v1/catalog/export" download variant="default" leftSection={<Download size={17} />}>{t('export')}</Button>
          <Button variant="default" leftSection={<Upload size={17} />} onClick={() => setImportOpen(!importOpen)}>{t('import')}</Button>
        </Group>
      </Group>
      {importOpen && (
        <Paper p="md" shadow="lg" style={{ position: 'absolute', right: 0, top: '100%', zIndex: 10, width: 'min(620px, calc(100vw - 40px))' }}>
          <form onSubmit={submit}>
            <Stack gap="md">
              <Textarea name="catalog" label={t('catalogJson')} rows={8} required />
              <Group justify="flex-end">
                <Button type="submit" loading={importCatalog.isPending}>{t('validateAndImport')}</Button>
              </Group>
            </Stack>
          </form>
        </Paper>
      )}
    </Paper>
  )
}

function CatalogOverrideEditor() {
  const { t } = useTranslation()
  const [kind, setKind] = useState('model')
  const [open, setOpen] = useState(false)
  const mutate = useMutation({ mutationFn: ({ id, document, remove }: { id: string; document?: unknown; remove?: boolean }) => api(`/api/admin/v1/catalog/overrides/${kind}/${encodeURIComponent(id)}`, { method: remove ? 'DELETE' : 'PUT', body: remove ? undefined : JSON.stringify(document) }), onSuccess: () => toast.success(t('saved')), onError: (error: Error) => toast.error(error.message) })
  const submit = (event: FormEvent<HTMLFormElement>) => { event.preventDefault(); const data = new FormData(event.currentTarget); try { const id = String(data.get('id')); const document = JSON.parse(String(data.get('document'))); mutate.mutate({ id, document: { ...document, id } }) } catch { toast.error(t('invalidJson')) } }
  return (
    <Paper p="md" withBorder>
      <UnstyledButton onClick={() => setOpen(!open)} w="100%" aria-expanded={open}>
        <Group justify="space-between">
          <Text fw={600}>{t('localOverrides')}</Text>
        </Group>
      </UnstyledButton>
      <Collapse expanded={open}>
        <form onSubmit={submit} style={{ marginTop: 14 }}>
          <Stack gap="md">
            <SelectField name="kind" label={t('type')} value={kind} onValueChange={setKind} options={[{ value: 'provider', label: t('provider') }, { value: 'model', label: t('model') }]} />
            <TextInput name="id" label={t('identifier')} required />
            <Textarea name="document" label={t('overrideJson')} rows={8} required />
            <Group justify="flex-end" gap="xs">
              <Button type="button" variant="outline" color="red" onClick={(event) => { const form = event.currentTarget.form; if (form) { const id = String(new FormData(form).get('id')); if (id) confirmAction({ title: t('deleteOverrideTitle', { id }), body: t('deleteOverrideBody', { kind: t(kind) }), onConfirm: () => mutate.mutateAsync({ id, remove: true }) }) } }}>{t('delete')}</Button>
              <Button type="submit" loading={mutate.isPending}>{t('save')}</Button>
            </Group>
          </Stack>
        </form>
      </Collapse>
    </Paper>
  )
}

type CatalogSource = {
  id: string
  name: string
  url: string
  priority: number
  refresh_interval_secs: number
  enabled: boolean
  signature_policy: string
  public_key: string | null
  revision: number
  last_attempt_at: number | null
  last_success_at: number | null
  last_error: string | null
  active_snapshot_id: string | null
  previous_snapshot_id: string | null
}
type CatalogSnapshot = { id: string; version: string; digest: string; source_url: string; signature_verified: boolean; created_at: number }
/** `POST /refresh` and `POST /refresh-due` answer with this, and the batch form adds `error`. */
type RefreshOutcome = { source_id: string; status: string; snapshot_id: string | null; error?: string }

/**
 * When the next scheduled refresh is due, derived the same way the scheduler
 * derives it: the last attempt plus the source interval. A disabled source is
 * never due, and a source that has never been attempted is due immediately.
 */
export function nextDueAt(source: CatalogSource, now: number): { dueAt: number | null; dueNow: boolean } {
  if (!source.enabled) return { dueAt: null, dueNow: false }
  if (!source.last_attempt_at) return { dueAt: null, dueNow: true }
  const dueAt = source.last_attempt_at + source.refresh_interval_secs
  return { dueAt, dueNow: dueAt <= now }
}

function CatalogNextDue({ source }: { source: CatalogSource }) {
  const { t } = useTranslation()
  if (!source.enabled) return <Text size="sm" c="dimmed">—</Text>
  const { dueAt, dueNow } = nextDueAt(source, Math.floor(Date.now() / 1000))
  if (dueNow) return <Text size="sm">{t('catalogDueNow')}</Text>
  return <Text size="sm">{formatDate(dueAt)}</Text>
}

/**
 * The active snapshot with its own time and signature verdict. The sources list
 * carries the snapshot id only, so the verdict comes from the snapshot list; a
 * failed lookup is reported as unavailable rather than as "no snapshot".
 */
function CatalogActiveSnapshot({ source }: { source: CatalogSource }) {
  const { t } = useTranslation()
  const query = useQuery({ queryKey: ['catalog-snapshots', source.id], queryFn: () => api<CatalogSnapshot[]>(`/api/admin/v1/catalog/sources/${source.id}/snapshots`) })
  if (!source.active_snapshot_id) return <Text size="sm" c="dimmed">—</Text>
  if (query.isError) {
    return (
      <Group gap={4} wrap="nowrap">
        <Text size="sm" c="dimmed">{t('catalogSnapshotUnknown')}</Text>
        <ActionIcon variant="subtle" color="gray" aria-label={t('retry')} onClick={() => void query.refetch()}><RefreshCw size={15} /></ActionIcon>
      </Group>
    )
  }
  if (query.isLoading) return <Loader size={14} aria-label={t('loading')} />
  const active = (query.data || []).find((snapshot) => snapshot.id === source.active_snapshot_id)
  if (!active) return <Text size="sm" c="dimmed">—</Text>
  return (
    <Stack gap={2}>
      <Text size="sm" className="mono-cell">{active.version}</Text>
      <Text size="sm" c="dimmed">{t('catalogSnapshotCreated', { time: formatDate(active.created_at) })}</Text>
      <Text size="sm" c={active.signature_verified ? undefined : 'dimmed'}>{active.signature_verified ? t('catalogSnapshotSignature') : t('catalogSnapshotUnsigned')}</Text>
    </Stack>
  )
}

/** Snapshot history for one source, with the active snapshot named as such. */
function CatalogSnapshots({ source, onRollback }: { source: CatalogSource; onRollback: (snapshot: CatalogSnapshot) => void }) {
  const { t } = useTranslation()
  const query = useQuery({ queryKey: ['catalog-snapshots', source.id], queryFn: () => api<CatalogSnapshot[]>(`/api/admin/v1/catalog/sources/${source.id}/snapshots`) })
  if (query.isError) return <QueryError retry={() => void query.refetch()} />
  if (query.isLoading) return <SkeletonRows count={2} />
  if (!query.data?.length) return <Text size="sm" c="dimmed">{t('noSnapshots')}</Text>
  return (
    <Stack gap="xs">
      {query.data.map((snapshot) => {
        const active = snapshot.id === source.active_snapshot_id
        return (
          <Group key={snapshot.id} justify="space-between" gap="sm" wrap="nowrap">
            <Stack gap={2} style={{ minWidth: 0 }}>
              <Text size="sm" className="mono-cell" style={{ overflowWrap: 'anywhere' }}>{snapshot.version} · {String(snapshot.digest).slice(0, 12)}</Text>
              <Text size="sm" c="dimmed">{t('catalogSnapshotCreated', { time: formatDate(snapshot.created_at) })}</Text>
              <Text size="sm" c={snapshot.signature_verified ? undefined : 'dimmed'}>{snapshot.signature_verified ? t('catalogSnapshotSignature') : t('catalogSnapshotUnsigned')}{active ? ` · ${t('catalogSnapshotActive')}` : ''}</Text>
            </Stack>
            <Tooltip label={t('catalogSnapshotAlreadyActive')} disabled={!active}>
              <span>
                <Button variant="default" size="compact-sm" disabled={active} onClick={() => onRollback(snapshot)}>{t('rollback')}</Button>
              </span>
            </Tooltip>
          </Group>
        )
      })}
    </Stack>
  )
}

function CatalogSubscriptions() {
  const { t } = useTranslation()
  const client = useQueryClient()
  const [interval, setInterval] = useAutoRefreshInterval('pangolin-auto-refresh-catalog')
  const [editing, setEditing] = useState<CatalogSource | null>(null)
  const [open, setOpen] = useState(false)
  const [snapshots, setSnapshots] = useState<CatalogSource | null>(null)
  const query = useQuery({ queryKey: ['catalog-sources'], queryFn: () => api<CatalogSource[]>('/api/admin/v1/catalog/sources'), refetchInterval: interval ?? false })
  const invalidate = () => void client.invalidateQueries({ queryKey: ['catalog-sources'] })
  const save = useMutation({ mutationFn: ({ source, current }: { source: Omit<CatalogSource, 'id' | 'revision' | 'last_attempt_at' | 'last_success_at' | 'last_error' | 'active_snapshot_id' | 'previous_snapshot_id'>; current: CatalogSource | null }) => api(current ? `/api/admin/v1/catalog/sources/${current.id}` : '/api/admin/v1/catalog/sources', { method: current ? 'PUT' : 'POST', body: JSON.stringify(current ? { revision: current.revision, source } : source) }), onSuccess: () => { invalidate(); setOpen(false) }, onError: (error: Error) => toast.error(error.message) })
  const refresh = useMutation({
    mutationFn: (id: string) => api<RefreshOutcome>(`/api/admin/v1/catalog/sources/${id}/refresh`, { method: 'POST', body: '{}' }),
    // The outcome is what the server recorded, so it is reported instead of a bare "refreshed".
    onSuccess: (outcome) => {
      if (outcome.status === 'activated') toast.success(t('catalogRefreshActivated', { snapshot: String(outcome.snapshot_id ?? '').slice(0, 12) }))
      else toast.success(t('catalogRefreshNotModified'))
      invalidate()
    },
    // A failed attempt is a recorded state change too (`last_attempt_at`, `last_error`),
    // so the row the server just wrote is re-read before the failure is reported.
    onError: (error: Error) => { invalidate(); toast.error(error.message) },
  })
  const refreshDue = useMutation({
    mutationFn: () => api<RefreshOutcome[]>('/api/admin/v1/catalog/sources/refresh-due', { method: 'POST', body: '{}' }),
    onSuccess: (outcomes) => {
      const failed = outcomes.filter((outcome) => outcome.status === 'failed').length
      const activated = outcomes.filter((outcome) => outcome.status === 'activated').length
      const refreshed = outcomes.length - failed
      if (outcomes.length === 0) toast.success(t('catalogRefreshDueNone'))
      else if (failed > 0) toast.error(t('catalogRefreshDueSummary', { refreshed, activated, failed }))
      else toast.success(t('catalogRefreshDueSummary', { refreshed, activated, failed }))
      invalidate()
    },
    // A batch that fails as a whole says nothing about the rows it may have touched, so
    // the list is re-read rather than left showing a state the console cannot vouch for.
    onError: (error: Error) => { invalidate(); toast.error(error.message) },
  })
  const remove = useMutation({ mutationFn: (id: string) => api(`/api/admin/v1/catalog/sources/${id}`, { method: 'DELETE' }), onSuccess: invalidate, onError: (error: Error) => toast.error(error.message) })
  const rollback = useMutation({ mutationFn: ({ source, snapshot }: { source: CatalogSource; snapshot: string }) => api(`/api/admin/v1/catalog/sources/${source.id}/rollback`, { method: 'POST', body: JSON.stringify({ revision: source.revision, snapshot_id: snapshot }) }), onSuccess: () => { invalidate(); void client.invalidateQueries({ queryKey: ['catalog-snapshots'] }); setSnapshots(null) }, onError: (error: Error) => toast.error(error.message) })
  const submit = (event: FormEvent<HTMLFormElement>) => { event.preventDefault(); const data = new FormData(event.currentTarget); save.mutate({ current: editing, source: { name: String(data.get('name')), url: String(data.get('url')), priority: Number(data.get('priority')), refresh_interval_secs: Number(data.get('refresh_interval_secs')), enabled: data.get('enabled') === 'on', signature_policy: String(data.get('signature_policy')), public_key: String(data.get('public_key') || '') || null } }) }
  return (
    <>
      <PageHeader
        title={t('subscriptions')}
        description={`${t('subscriptionsDescription')} ${t('catalogSyncHint')}`}
        action={
          <Group gap="xs" wrap="wrap" justify="flex-end">
            <AutoRefreshControl interval={interval} onIntervalChange={setInterval} onRefresh={() => query.refetch()} />
            <Button variant="default" leftSection={<RefreshCw size={17} />} loading={refreshDue.isPending} onClick={() => refreshDue.mutate()}>{t('catalogRefreshAllDue')}</Button>
            <Button leftSection={<Plus size={17} />} onClick={() => { setEditing(null); setOpen(true) }}>{t('addSubscription')}</Button>
          </Group>
        }
      />
      {query.isError ? <QueryError retry={() => void query.refetch()} /> : query.isLoading ? <SkeletonRows /> : query.data && query.data.length > 0 ? (
        <TableScrollContainer minWidth={1100} style={{ maxHeight: 'min(74vh, 900px)' }}>
          <Table stickyHeader highlightOnHover>
            <Table.Thead>
              <Table.Tr>
                <Table.Th>{t('name')}</Table.Th>
                <Table.Th>{t('priority')}</Table.Th>
                <Table.Th>{t('status')}</Table.Th>
                <Table.Th>{t('catalogLastAttempt')}</Table.Th>
                <Table.Th>{t('catalogLastSuccess')}</Table.Th>
                <Table.Th>{t('catalogNextDue')}</Table.Th>
                <Table.Th>{t('catalogActiveSnapshot')}</Table.Th>
                <Table.Th>{t('lastError')}</Table.Th>
                <Table.Th><span className="sr-only">{t('actions')}</span></Table.Th>
              </Table.Tr>
            </Table.Thead>
            <Table.Tbody>
              {query.data.map((source) => (
                <Table.Tr key={source.id}>
                  <Table.Td>
                    <Stack gap={2}>
                      <strong>{source.name}</strong>
                      <Text size="sm" c="dimmed" className="mono-cell" style={{ overflowWrap: 'anywhere' }}>{source.url}</Text>
                    </Stack>
                  </Table.Td>
                  <Table.Td>{source.priority}</Table.Td>
                  <Table.Td><EnabledPill enabled={source.enabled} /></Table.Td>
                  <Table.Td>{formatDate(source.last_attempt_at)}</Table.Td>
                  <Table.Td>{formatDate(source.last_success_at)}</Table.Td>
                  <Table.Td><CatalogNextDue source={source} /></Table.Td>
                  <Table.Td><CatalogActiveSnapshot source={source} /></Table.Td>
                  <Table.Td>{source.last_error || '—'}</Table.Td>
                  <Table.Td>
                    <Group gap={4} justify="flex-end" wrap="nowrap">
                      <ActionIcon variant="subtle" color="gray" loading={refresh.isPending && refresh.variables === source.id} aria-label={`${t('refresh')} ${source.name}`} onClick={() => refresh.mutate(source.id)}><RefreshCw size={16} /></ActionIcon>
                      <Button variant="subtle" size="compact-sm" onClick={() => { setEditing(source); setOpen(true) }}>{t('edit')}</Button>
                      <ActionIcon variant="subtle" color="gray" aria-label={`${t('rollback')} ${source.name}`} onClick={() => setSnapshots(source)}><RotateCcw size={16} /></ActionIcon>
                      <ActionIcon variant="subtle" color="red" aria-label={`${t('delete')} ${source.name}`} onClick={() => confirmAction({ title: t('deleteSubscriptionTitle', { name: source.name }), body: t('deleteSubscriptionBody'), onConfirm: () => remove.mutateAsync(source.id) })}><Trash2 size={16} /></ActionIcon>
                    </Group>
                  </Table.Td>
                </Table.Tr>
              ))}
            </Table.Tbody>
          </Table>
        </TableScrollContainer>
      ) : (
        <Text size="sm" c="dimmed">{t('subscriptionEmpty')}</Text>
      )}
      <Modal open={open} onOpenChange={setOpen} title={editing ? t('editSubscription') : t('addSubscription')}>
        <form onSubmit={submit}>
          <Stack gap="md">
            <TextInput name="name" label={t('name')} required defaultValue={editing?.name} />
            <TextInput name="url" label={t('httpsUrl')} type="url" required defaultValue={editing?.url} />
            <Group gap="sm" wrap="wrap">
              <NumberInput name="priority" label={t('priority')} required defaultValue={editing?.priority ?? 100} hideControls style={{ flex: 1 }} />
              <NumberInput name="refresh_interval_secs" label={t('intervalSeconds')} min={60} max={2592000} required defaultValue={editing?.refresh_interval_secs ?? 3600} hideControls style={{ flex: 1 }} />
            </Group>
            <Select name="signature_policy" label={t('signaturePolicy')} defaultValue={editing?.signature_policy || 'none'} data={['none', 'optional', 'required'].map((value) => ({ value, label: t(value) }))} />
            <TextInput name="public_key" label={t('publicKey')} defaultValue={editing?.public_key || ''} />
            <Checkbox name="enabled" label={t('enabled')} defaultChecked={editing?.enabled ?? true} />
            <Group justify="flex-end">
              <Button type="submit" loading={save.isPending}>{t('save')}</Button>
            </Group>
          </Stack>
        </form>
      </Modal>
      <Modal open={Boolean(snapshots)} onOpenChange={(value) => !value && setSnapshots(null)} title={t('rollback')} description={t('rollbackHint')}>
        {snapshots && <CatalogSnapshots source={snapshots} onRollback={(snapshot) => confirmAction({ title: t('rollbackSnapshotTitle', { name: snapshots.name, version: snapshot.version }), body: t('rollbackSnapshotBody'), confirmLabel: t('rollback'), onConfirm: () => rollback.mutateAsync({ source: snapshots, snapshot: snapshot.id }) })} />}
      </Modal>
    </>
  )
}
