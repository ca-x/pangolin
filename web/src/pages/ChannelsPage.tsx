import { Button, Card, Group, Paper, Select, SimpleGrid, Stack, Tabs, Text, TextInput, Title } from '@mantine/core'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Plus } from 'lucide-react'
import { useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { api, type Document, type Paged } from '../api'
import { EnabledPill, SelectField, SkeletonRows } from '../components'
import { ProviderIcon } from '../ProviderIcon'
import { projectOperationPath, useProject } from '../project'
import { PageHeader, QueryError, ResourcePage } from './shared'

const CHANNEL_TABS = ['channels', 'credentials', 'channelPolicies', 'probes', 'quotas', 'presets'] as const

export default function ChannelsPage() {
  const { t } = useTranslation()
  return (
    <Tabs keepMounted={false} defaultValue="channels">
      <Tabs.List mb="lg">
        {CHANNEL_TABS.map((value) => (
          <Tabs.Tab key={value} value={value}>{t(value)}</Tabs.Tab>
        ))}
      </Tabs.List>
      <Tabs.Panel value="channels">
        <ResourcePage resource="channels" title={t('channels')} description={t('channelsDescription')} empty={t('channelEmpty')} createLabel={t('addChannel')} columns={[{ key: 'name', label: t('name'), render: (value, row) => <span className="provider-cell"><ProviderIcon logoKey={String((row.settings as Record<string, unknown>)?.logo_key || '')} name={String(value)} /><strong>{String(value)}</strong></span> }, { key: 'kind', label: t('providerType') }, { key: 'base_url', label: t('baseUrl'), mono: true }, { key: 'enabled', label: t('status'), render: (value) => <EnabledPill enabled={value} /> }]} fields={[{ key: 'name', label: t('name'), required: true }, { key: 'kind', label: t('providerType'), kind: 'select', required: true, options: providerKinds() }, { key: 'base_url', label: t('baseUrl'), required: true, defaultValue: 'https://api.openai.com' }, { key: 'settings', label: t('advancedSettings'), kind: 'json', defaultValue: { version: 1, tags: [], limits: {}, circuit: { failures: 5, window_ms: 60000, recovery_ms: 30000 } } }, { key: 'enabled', label: t('status'), kind: 'checkbox' }]} />
        <BulkToggle />
      </Tabs.Panel>
      <Tabs.Panel value="credentials"><CredentialsPanel /></Tabs.Panel>
      <Tabs.Panel value="channelPolicies"><ChannelSettingsPanel /></Tabs.Panel>
      <Tabs.Panel value="probes">
        <Diagnostics />
        <ResourcePage resource="probes" title={t('probes')} description={t('probesDescription')} empty={t('probeEmpty')} immutable columns={[{ key: 'provider_id', label: t('channel'), mono: true }, { key: 'success', label: t('status') }, { key: 'status_code', label: t('statusCode') }, { key: 'latency_ms', label: t('latency') }, { key: 'probed_at', label: t('time') }]} />
      </Tabs.Panel>
      <Tabs.Panel value="quotas">
        <Diagnostics quota />
        <ResourcePage resource="quotas" title={t('quotas')} description={t('quotasDescription')} empty={t('quotaEmpty')} immutable columns={[{ key: 'provider_id', label: t('channel'), mono: true }, { key: 'remaining_micros', label: t('remaining') }, { key: 'period_end', label: t('periodEnd') }, { key: 'collected_at', label: t('time') }]} />
      </Tabs.Panel>
      <Tabs.Panel value="presets"><PresetsPanel /></Tabs.Panel>
    </Tabs>
  )
}

function providerKinds() {
  return ['openai', 'openai_compatible', 'anthropic', 'gemini', 'azure', 'bedrock', 'vertex', 'gcp', 'openrouter', 'deepseek', 'moonshot', 'zhipu', 'doubao', 'xai', 'groq', 'ollama', 'nanogpt', 'jina'].map((value) => ({ value, label: value.replaceAll('_', ' ') }))
}

function useChannelOptions() {
  const { project } = useProject()
  const query = useQuery({ queryKey: ['channel-options', project.id], queryFn: () => api<Paged<Document>>(projectOperationPath(project.id, 'channels') + '?limit=500') })
  return { query, options: (query.data?.data || []).map((row) => ({ value: String(row.id), label: String(row.name) })) }
}

function CredentialsPanel() {
  const { t } = useTranslation()
  const { options } = useChannelOptions()
  return (
    <>
      <ResourcePage resource="credentials" title={t('credentials')} description={t('credentialsDescription')} empty={t('credentialEmpty')} createLabel={t('addCredential')} columns={[{ key: 'provider_name', label: t('channel') }, { key: 'suffix', label: t('suffix'), mono: true }, { key: 'credential_type', label: t('credentialType') }, { key: 'priority', label: t('priority') }, { key: 'enabled', label: t('status'), render: (value) => <EnabledPill enabled={value} /> }]} fields={[{ key: 'provider_id', label: t('channel'), kind: 'select', required: true, options }, { key: 'credential_type', label: t('credentialType'), defaultValue: 'api_key', required: true }, { key: 'secret', label: t('secret'), kind: 'secret', hint: t('secretUpdateHint'), omitWhenBlank: true }, { key: 'priority', label: t('priority'), kind: 'number', defaultValue: 100 }, { key: 'enabled', label: t('status'), kind: 'checkbox' }, { key: 'settings', label: t('advancedSettings'), kind: 'json', defaultValue: { version: 1 } }]} />
      <BulkToggle resource="credentials" />
    </>
  )
}

function ChannelSettingsPanel() {
  const { t } = useTranslation()
  const { options } = useChannelOptions()
  return (
    <ResourcePage resource="channel-settings" title={t('channelPolicies')} description={t('channelPoliciesHint')} empty={t('channelPoliciesEmpty')} canDelete={false} columns={[{ key: 'provider_id', label: t('channel'), render: (value) => options.find((option) => option.value === value)?.label || String(value) }, { key: 'retry_statuses', label: t('retryStatuses') }, { key: 'auto_disable_policy', label: t('autoDisable') }]} fields={[{ key: 'provider_id', label: t('channel'), kind: 'select', required: true, options }, { key: 'endpoint_mappings', label: t('endpointMappings'), kind: 'json', defaultValue: { version: 1 } }, { key: 'model_rules', label: t('modelRules'), kind: 'json', defaultValue: { version: 1 } }, { key: 'parameter_overrides', label: t('parameterOverrides'), kind: 'json', defaultValue: { version: 1 } }, { key: 'retry_statuses', label: t('retryStatuses'), kind: 'json', defaultValue: { version: 1, statuses: [408, 409, 429, 500, 502, 503, 504] } }, { key: 'auto_disable_policy', label: t('autoDisable'), kind: 'json', defaultValue: { version: 1, enabled: false } }]} />
  )
}

function BulkToggle({ resource = 'channels' }: { resource?: 'channels' | 'credentials' }) {
  const { t } = useTranslation()
  const { project } = useProject()
  const client = useQueryClient()
  const mutate = useMutation({ mutationFn: (body: unknown) => api(projectOperationPath(project.id, 'bulk-toggle'), { method: 'POST', body: JSON.stringify(body) }), onSuccess: () => { toast.success(t('saved')); void client.invalidateQueries({ queryKey: ['resource', project.id, resource] }) }, onError: (error: Error) => toast.error(error.message) })
  const submit = (event: FormEvent<HTMLFormElement>) => { event.preventDefault(); const data = new FormData(event.currentTarget); mutate.mutate({ resource, ids: String(data.get('ids')).split(',').map((id) => id.trim()).filter(Boolean), enabled: data.get('enabled') === 'true' }) }
  return (
    <Paper withBorder p="md" mt="md">
      <Title order={3} mb="md">{t('bulkActions')}</Title>
      <form onSubmit={submit}>
        <Group gap="md" align="end" wrap="wrap">
          <TextInput name="ids" label={t('resourceIds')} placeholder="id-1, id-2" required style={{ minWidth: 260 }} />
          <Select name="enabled" label={t('action')} defaultValue="true" data={[{ value: 'true', label: t('enable') }, { value: 'false', label: t('disable') }]} />
          <Button type="submit">{t('apply')}</Button>
        </Group>
      </form>
    </Paper>
  )
}

function Diagnostics({ quota = false }: { quota?: boolean }) {
  const { t } = useTranslation()
  const { project } = useProject()
  const { options } = useChannelOptions()
  const [selected, setSelected] = useState('')
  const run = useMutation({ mutationFn: (provider_id: string) => api(projectOperationPath(project.id, quota ? 'quota' : 'probe'), { method: 'POST', body: JSON.stringify({ provider_id }) }), onSuccess: () => toast.success(t('jobQueued')), onError: (error: Error) => toast.error(error.message) })
  const current = selected || options[0]?.value || ''
  return (
    <Paper withBorder p="md" mb="md">
      <form onSubmit={(event) => { event.preventDefault(); run.mutate(current) }}>
        <Group gap="md" align="end" wrap="wrap">
          <SelectField label={t('channel')} value={current} onValueChange={setSelected} options={options} />
          <Button type="submit" disabled={!current}>{quota ? t('collectQuota') : t('runProbe')}</Button>
        </Group>
      </form>
    </Paper>
  )
}

function PresetsPanel() {
  const { t } = useTranslation()
  const { project } = useProject()
  const client = useQueryClient()
  const query = useQuery({ queryKey: ['catalog-providers'], queryFn: () => api<Paged<Document>>('/api/admin/v1/catalog/providers?limit=200') })
  const create = useMutation({ mutationFn: (preset: Document) => api(projectOperationPath(project.id, 'channels'), { method: 'POST', body: JSON.stringify({ name: preset.name, kind: preset.adapter_kind, base_url: preset.default_base_url, enabled: true, settings: { version: 1, catalog_provider_id: preset.id, logo_key: preset.logo_key } }) }), onSuccess: () => { toast.success(t('saved')); void client.invalidateQueries({ queryKey: ['resource', project.id, 'channels'] }) }, onError: (error: Error) => toast.error(error.message) })
  if (query.isError) return <QueryError retry={() => void query.refetch()} />
  if (query.isLoading) return <SkeletonRows />
  return (
    <>
      <PageHeader title={t('presets')} description={t('presetsDescription')} />
      <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="sm">
        {(query.data?.data || []).map((preset) => (
          <Card key={preset.id} padding="md">
            <Group gap="md" wrap="nowrap">
              <ProviderIcon logoKey={String(preset.logo_key || '')} name={String(preset.name)} />
              <Stack gap={4} style={{ flex: 1, minWidth: 0 }}>
                <Title order={3}>{String(preset.name)}</Title>
                <Text size="sm" c="dimmed" ff="mono">{String(preset.default_base_url || '—')}</Text>
              </Stack>
              <Button
                leftSection={<Plus size={16} />}
                disabled={!preset.adapter_available || !preset.adapter_kind || !preset.default_base_url || create.isPending}
                onClick={() => create.mutate(preset)}
              >
                {preset.adapter_available ? t('usePreset') : t('catalogOnly')}
              </Button>
            </Group>
          </Card>
        ))}
      </SimpleGrid>
    </>
  )
}