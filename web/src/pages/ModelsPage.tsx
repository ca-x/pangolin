import { ActionIcon, Button, Checkbox, Collapse, Group, NumberInput, Paper, Select, Stack, Table, TableScrollContainer, Tabs, Text, Textarea, TextInput, Title, UnstyledButton } from '@mantine/core'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Download, Plus, RefreshCw, RotateCcw, Trash2, Upload } from 'lucide-react'
import { useEffect, useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { api, type Document, type Paged } from '../api'
import { CapabilityTags, EnabledPill, Modal, SelectField, SkeletonRows } from '../components'
import { useProject } from '../project'
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
      <Tabs.Panel value="catalog"><CatalogMaintenance /><CatalogOverrideEditor /><ResourcePage resource="catalog-models" endpoint="/api/admin/v1/catalog/models" title={t('catalog')} description={t('catalogDescription')} empty={t('catalogEmpty')} immutable columns={[{ key: 'id', label: t('modelId'), mono: true }, { key: 'name', label: t('name'), mono: true }, { key: 'type', label: t('type') }, { key: 'capabilities', label: t('capabilities'), render: (value) => <CapabilityTags value={value} /> }, { key: 'provider_id', label: t('provider') }]} /></Tabs.Panel>
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
  }
}

function ModelsPanel() {
  const { t } = useTranslation()
  const { channels } = useModelRelations()
  return <ResourcePage resource="models" title={t('models')} description={t('modelsDescription')} empty={t('modelEmpty')} createLabel={t('addModel')} columns={[{ key: 'public_name', label: t('publicModel'), mono: true }, { key: 'provider_name', label: t('channel') }, { key: 'upstream_name', label: t('upstreamModel'), mono: true }, { key: 'capabilities', label: t('capabilities'), render: (value) => <CapabilityTags value={value} /> }, { key: 'enabled', label: t('status'), render: (value) => <EnabledPill enabled={value} /> }]} fields={[{ key: 'provider_id', label: t('channel'), kind: 'select', required: true, options: channels }, { key: 'public_name', label: t('publicModel'), required: true }, { key: 'upstream_name', label: t('upstreamModel'), required: true }, { key: 'capabilities', label: t('capabilities'), kind: 'json', defaultValue: ['chat', 'responses'] }, { key: 'input_price_micros', label: t('inputPrice'), kind: 'number', defaultValue: 0 }, { key: 'output_price_micros', label: t('outputPrice'), kind: 'number', defaultValue: 0 }, { key: 'priority', label: t('priority'), kind: 'number', defaultValue: 100 }, { key: 'enabled', label: t('status'), kind: 'checkbox' }]} />
}

function RoutingEditor() {
  const { t } = useTranslation()
  const { channels, models } = useModelRelations()
  const any = { value: '__any__', label: t('unrestricted') }
  return <ResourcePage resource="associations" title={t('routing')} description={t('routingDescription')} empty={t('routingEmpty')} createLabel={t('addRoute')} columns={[{ key: 'pattern', label: t('pattern'), mono: true }, { key: 'match_type', label: t('matchType') }, { key: 'model_name', label: t('model'), render: (value) => value ? String(value) : t('unrestricted') }, { key: 'provider_name', label: t('channel'), render: (value) => value ? String(value) : t('unrestricted') }, { key: 'priority', label: t('priority') }, { key: 'enabled', label: t('status'), render: (value) => <EnabledPill enabled={value} /> }]} fields={[{ key: 'model_id', label: t('model'), kind: 'select', defaultValue: '__any__', options: [any, ...models] }, { key: 'provider_id', label: t('channel'), kind: 'select', defaultValue: '__any__', options: [any, ...channels] }, { key: 'match_type', label: t('matchType'), kind: 'select', options: ['exact', 'regex', 'tag'].map((value) => ({ value, label: value })) }, { key: 'pattern', label: t('pattern'), required: true }, { key: 'conditions', label: t('conditions'), kind: 'json', defaultValue: { version: 1 } }, { key: 'priority', label: t('priority'), kind: 'number', defaultValue: 100 }, { key: 'weight', label: t('weight'), kind: 'number', defaultValue: 1 }, { key: 'enabled', label: t('status'), kind: 'checkbox' }]} normalize={(values, editing) => ({ ...values, ...(editing ? { id: editing.id } : {}), model_id: values.model_id === '__any__' ? null : values.model_id, provider_id: values.provider_id === '__any__' ? null : values.provider_id })} />
}

function PricesPanel() {
  const { t } = useTranslation()
  const { models } = useModelRelations()
  return <ResourcePage resource="prices" title={t('prices')} description={t('pricesDescription')} empty={t('priceEmpty')} createLabel={t('addPrice')} appendOnly columns={[{ key: 'model_name', label: t('model') }, { key: 'version', label: t('version') }, { key: 'valid_from', label: t('validFrom'), render: formatDate }, { key: 'valid_until', label: t('validUntil'), render: formatDate }, { key: 'schedule', label: t('schedule') }]} fields={[{ key: 'model_id', label: t('model'), kind: 'select', required: true, options: models }, { key: 'components', label: t('priceComponents'), kind: 'json', required: true, defaultValue: [{ kind: 'input', unit_size: 1000000, unit_price_micros: 0 }] }, { key: 'schedule', label: t('schedule'), kind: 'json', defaultValue: { version: 1, rules: [] } }]} />
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
            <SelectField name="endpoint" label={t('endpoint')} value={endpoint} onValueChange={setEndpoint} options={[{ value: '/v1/chat/completions', label: 'OpenAI chat' }, { value: '/v1/responses', label: 'OpenAI responses' }, { value: '/v1/messages', label: 'Anthropic messages' }]} />
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
    <Paper p="md" withBorder mt="lg">
      <UnstyledButton onClick={() => setOpen(!open)} w="100%" aria-expanded={open}>
        <Group justify="space-between">
          <Text fw={600}>{t('localOverrides')}</Text>
        </Group>
      </UnstyledButton>
      <Collapse expanded={open}>
        <form onSubmit={submit} style={{ marginTop: 14 }}>
          <Stack gap="md">
            <SelectField name="kind" label={t('type')} value={kind} onValueChange={setKind} options={[{ value: 'provider', label: t('provider') }, { value: 'model', label: t('model') }]} />
            <TextInput name="id" label="ID" required />
            <Textarea name="document" label={t('overrideJson')} rows={8} required />
            <Group justify="flex-end" gap="xs">
              <Button type="button" variant="outline" color="red" onClick={(event) => { const form = event.currentTarget.form; if (form) { const id = String(new FormData(form).get('id')); if (id && confirm(t('deleteConfirm'))) mutate.mutate({ id, remove: true }) } }}>{t('delete')}</Button>
              <Button type="submit" loading={mutate.isPending}>{t('save')}</Button>
            </Group>
          </Stack>
        </form>
      </Collapse>
    </Paper>
  )
}

type CatalogSource = { id: string; name: string; url: string; priority: number; refresh_interval_secs: number; enabled: boolean; signature_policy: string; public_key: string | null; revision: number; last_success_at: number | null; last_error: string | null }

function CatalogSubscriptions() {
  const { t } = useTranslation()
  const client = useQueryClient()
  const [editing, setEditing] = useState<CatalogSource | null>(null)
  const [open, setOpen] = useState(false)
  const [snapshots, setSnapshots] = useState<{ source: CatalogSource; data: Document[] } | null>(null)
  const query = useQuery({ queryKey: ['catalog-sources'], queryFn: () => api<CatalogSource[]>('/api/admin/v1/catalog/sources') })
  const invalidate = () => void client.invalidateQueries({ queryKey: ['catalog-sources'] })
  const save = useMutation({ mutationFn: ({ source, current }: { source: Omit<CatalogSource, 'id' | 'revision' | 'last_success_at' | 'last_error'>; current: CatalogSource | null }) => api(current ? `/api/admin/v1/catalog/sources/${current.id}` : '/api/admin/v1/catalog/sources', { method: current ? 'PUT' : 'POST', body: JSON.stringify(current ? { revision: current.revision, source } : source) }), onSuccess: () => { invalidate(); setOpen(false) }, onError: (error: Error) => toast.error(error.message) })
  const refresh = useMutation({ mutationFn: (id: string) => api(`/api/admin/v1/catalog/sources/${id}/refresh`, { method: 'POST', body: '{}' }), onSuccess: invalidate, onError: (error: Error) => toast.error(error.message) })
  const remove = useMutation({ mutationFn: (id: string) => api(`/api/admin/v1/catalog/sources/${id}`, { method: 'DELETE' }), onSuccess: invalidate, onError: (error: Error) => toast.error(error.message) })
  const loadSnapshots = async (source: CatalogSource) => setSnapshots({ source, data: await api<Document[]>(`/api/admin/v1/catalog/sources/${source.id}/snapshots`) })
  const rollback = useMutation({ mutationFn: ({ source, snapshot }: { source: CatalogSource; snapshot: string }) => api(`/api/admin/v1/catalog/sources/${source.id}/rollback`, { method: 'POST', body: JSON.stringify({ revision: source.revision, snapshot_id: snapshot }) }), onSuccess: () => { invalidate(); setSnapshots(null) }, onError: (error: Error) => toast.error(error.message) })
  const submit = (event: FormEvent<HTMLFormElement>) => { event.preventDefault(); const data = new FormData(event.currentTarget); save.mutate({ current: editing, source: { name: String(data.get('name')), url: String(data.get('url')), priority: Number(data.get('priority')), refresh_interval_secs: Number(data.get('refresh_interval_secs')), enabled: data.get('enabled') === 'on', signature_policy: String(data.get('signature_policy')), public_key: String(data.get('public_key') || '') || null } }) }
  return (
    <>
      <PageHeader title={t('subscriptions')} description={t('subscriptionsDescription')} action={<Button leftSection={<Plus size={17} />} onClick={() => { setEditing(null); setOpen(true) }}>{t('addSubscription')}</Button>} />
      {query.isError ? <QueryError retry={() => void query.refetch()} /> : query.isLoading ? <SkeletonRows /> : query.data && query.data.length > 0 ? (
        <TableScrollContainer minWidth={700} style={{ maxHeight: 'min(74vh, 900px)' }}>
          <Table stickyHeader highlightOnHover>
            <Table.Thead>
              <Table.Tr>
                <Table.Th>{t('name')}</Table.Th>
                <Table.Th>URL</Table.Th>
                <Table.Th>{t('priority')}</Table.Th>
                <Table.Th>{t('status')}</Table.Th>
                <Table.Th>{t('lastError')}</Table.Th>
                <Table.Th><span className="sr-only">{t('actions')}</span></Table.Th>
              </Table.Tr>
            </Table.Thead>
            <Table.Tbody>
              {query.data.map((source) => (
                <Table.Tr key={source.id}>
                  <Table.Td><strong>{source.name}</strong></Table.Td>
                  <Table.Td><code>{source.url}</code></Table.Td>
                  <Table.Td>{source.priority}</Table.Td>
                  <Table.Td>{source.enabled ? t('enabled') : t('disabled')}</Table.Td>
                  <Table.Td>{source.last_error || '—'}</Table.Td>
                  <Table.Td>
                    <Group gap={4} justify="flex-end" wrap="nowrap">
                      <ActionIcon variant="subtle" color="gray" aria-label={`${t('refresh')} ${source.name}`} onClick={() => refresh.mutate(source.id)}><RefreshCw size={16} /></ActionIcon>
                      <Button variant="subtle" size="compact-sm" onClick={() => { setEditing(source); setOpen(true) }}>{t('edit')}</Button>
                      <ActionIcon variant="subtle" color="gray" aria-label={`${t('rollback')} ${source.name}`} onClick={() => void loadSnapshots(source)}><RotateCcw size={16} /></ActionIcon>
                      <ActionIcon variant="subtle" color="red" aria-label={`${t('delete')} ${source.name}`} onClick={() => confirm(t('deleteConfirm')) && remove.mutate(source.id)}><Trash2 size={16} /></ActionIcon>
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
            <TextInput name="url" label="HTTPS URL" type="url" required defaultValue={editing?.url} />
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
        <Stack gap="xs">
          {snapshots?.data.length
            ? snapshots.data.map((snapshot) => (
              <Button key={snapshot.id} variant="default" onClick={() => rollback.mutate({ source: snapshots.source, snapshot: String(snapshot.id) })}>
                {displayValue(snapshot.version)} · {String(snapshot.digest).slice(0, 12)}
              </Button>
            ))
            : <Text size="sm" c="dimmed">{t('noSnapshots')}</Text>}
        </Stack>
      </Modal>
    </>
  )
}