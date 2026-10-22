import { Badge, Button, Group, Paper, SimpleGrid, Stack, Text, Title } from '@mantine/core'
import { useIsMutating, useMutation, useQueryClient } from '@tanstack/react-query'
import { AlertTriangle, Check, Circle, Power, RefreshCw } from 'lucide-react'
import { useId } from 'react'
import { useTranslation } from 'react-i18next'
import { api } from '../api'
import { formatDate } from './shared'

export type CatalogSource = {
  id: string
  name: string
  url: string
  priority: number
  refresh_interval_secs: number
  enabled: boolean
  signature_policy: string
  public_key: string | null
  proxy_preset_id?: string | null
  revision: number
  last_attempt_at: number | null
  last_success_at: number | null
  last_error: string | null
  active_snapshot_id: string | null
  previous_snapshot_id: string | null
}

const endpoint = '/api/admin/v1/catalog/sources'
const officialUrl = 'https://models.dev/api.json'
const preset = { name: 'models.dev', url: officialUrl, priority: 50, enabled: true, refresh_interval_secs: 86400, signature_policy: 'none', public_key: null }
const instantActionStyle = { transform: 'none', transition: 'none' }
const mutationKey = ['catalog-sources', 'models-dev']
type Action = 'connect' | 'refresh' | 'disable' | 'enable'
type RefreshOutcome = { source_id: string; status: string; snapshot_id: string | null; error?: string }

function officialSource(sources: CatalogSource[]) {
  return sources.find((source) => {
    try { return new URL(source.url).href === officialUrl } catch { return false }
  })
}

function sourceInput(source: CatalogSource, enabled: boolean) {
  return {
    name: source.name, url: source.url, priority: source.priority,
    enabled, refresh_interval_secs: source.refresh_interval_secs,
    signature_policy: source.signature_policy, public_key: source.public_key,
    ...(source.proxy_preset_id !== undefined ? { proxy_preset_id: source.proxy_preset_id } : {}),
  }
}

export function ModelsDevSubscription({ sources, onEdit }: { sources: CatalogSource[]; onEdit: (source: CatalogSource) => void }) {
  const { t } = useTranslation()
  const titleId = useId()
  const client = useQueryClient()
  const source = officialSource(sources)
  const pendingMutations = useIsMutating({ mutationKey })
  const sync = useMutation({
    mutationKey,
    retry: false,
    mutationFn: async (action: Action) => {
      const currentSources = await api<CatalogSource[]>(endpoint)
      let current = officialSource(currentSources)
      if (action === 'disable') {
        if (!current) throw new Error(t('modelsDevSourceMissing'))
        await api<CatalogSource>(`${endpoint}/${current.id}`, { method: 'PUT', body: JSON.stringify({ revision: current.revision, source: sourceInput(current, false) }) })
        return null
      }
      if (!current) {
        if (action !== 'connect') throw new Error(t('modelsDevSourceMissing'))
        current = await api<CatalogSource>(endpoint, { method: 'POST', body: JSON.stringify(preset) })
      }
      if (!current.enabled) {
        if (action === 'refresh') throw new Error(t('modelsDevSourceDisabled'))
        current = await api<CatalogSource>(`${endpoint}/${current.id}`, { method: 'PUT', body: JSON.stringify({ revision: current.revision, source: sourceInput(current, true) }) })
      }
      const outcome = await api<RefreshOutcome>(`${endpoint}/${current.id}/refresh`, { method: 'POST', body: '{}' })
      if (outcome.status === 'failed') throw new Error(outcome.error || t('modelsDevSyncFailed'))
      return outcome
    },
    onSettled: async () => {
      await Promise.all([
        client.invalidateQueries({ queryKey: ['catalog-sources'] }),
        client.invalidateQueries({ queryKey: ['catalog-snapshots'] }),
        client.invalidateQueries({ queryKey: ['resource'] }),
        client.invalidateQueries({ queryKey: ['model-create-catalog'] }),
        client.invalidateQueries({ queryKey: ['model-import-catalog'] }),
      ])
    },
  })
  const pending = sync.isPending || pendingMutations > 0
  const state = !source ? 'modelsDevNotConnected' : !source.enabled ? 'modelsDevDisabled' : source.last_error ? 'modelsDevNeedsAttention' : source.active_snapshot_id ? 'modelsDevReady' : 'modelsDevWaiting'
  const StateIcon = state === 'modelsDevNeedsAttention' ? AlertTriangle : state === 'modelsDevDisabled' ? Power : state === 'modelsDevReady' ? Check : Circle
  const color = state === 'modelsDevNeedsAttention' ? 'yellow' : state === 'modelsDevReady' ? 'green' : 'gray'
  const primaryAction = !source ? 'connect' : source.enabled ? 'refresh' : 'enable'
  const primaryLabel = !source ? 'modelsDevConnect' : !source.enabled ? 'modelsDevEnable' : source.last_error ? 'modelsDevRetrySync' : 'modelsDevSyncNow'

  return (
    <Paper component="section" aria-labelledby={titleId} withBorder p="md" mb="lg" style={{ minWidth: 0 }}>
      <Stack gap="md">
        <Group justify="space-between" gap="sm" wrap="wrap">
          <Title order={2} id={titleId}>{t('modelsDevTitle')}</Title>
          <Badge size="lg" fz="md" variant="light" color={color} leftSection={<StateIcon size={14} aria-hidden="true" />} styles={{ label: { textTransform: 'none' } }}>{t(state)}</Badge>
        </Group>
        <Text fz={16}>{t('modelsDevDescription')}</Text>
        <Text fz="md" c="dimmed">{t('modelsDevReferenceOnly')}</Text>
        <Text fz="md" c="dimmed" className="mono-cell" style={{ overflowWrap: 'anywhere' }}>{officialUrl}</Text>
        {!source ? <Text fz="md" c="dimmed">{t('modelsDevOptIn')}</Text> : <>
          <Text fz="md" c="dimmed">{t('modelsDevSchedule', { interval: source.refresh_interval_secs === 86400 ? t('modelsDevDaily') : t('modelsDevInterval', { seconds: source.refresh_interval_secs }) })}</Text>
          <Text fz="md" c="dimmed">{t('modelsDevDisableEffect')}</Text>
          <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="sm">
            <Stack gap={2}><Text fz="md" c="dimmed">{t('modelsDevLastSync')}</Text><Text fz="md">{formatDate(source.last_success_at)}</Text></Stack>
            <Stack gap={2}><Text fz="md" c="dimmed">{t('modelsDevSavedSnapshot')}</Text><Text fz="md" className="mono-cell" style={{ overflowWrap: 'anywhere' }}>{source.active_snapshot_id || '—'}</Text></Stack>
          </SimpleGrid>
          {source.last_error && <Stack gap={4} role="status">
            <Text fz="md" c="red" style={{ overflowWrap: 'anywhere' }}>{t('modelsDevLastAttemptError', { error: source.last_error })}</Text>
            <Text fz="md">{t(source.active_snapshot_id ? source.enabled ? 'modelsDevLastGood' : 'modelsDevSavedLastGood' : 'modelsDevNoMetadata')}</Text>
          </Stack>}
          {!source.active_snapshot_id && !source.last_error && <Text fz="md" c="dimmed">{t('modelsDevNoMetadata')}</Text>}
        </>}
        {sync.isError && <Paper withBorder p="sm" role="alert">
          <Stack gap="xs">
            <Text fz="md" fw={600}>{t('modelsDevActionFailed')}</Text>
            <Text fz="md" style={{ overflowWrap: 'anywhere' }}>{sync.error.message}</Text>
            <Group><Button variant="default" mih={44} style={instantActionStyle} disabled={pending} onClick={() => sync.variables && sync.mutate(sync.variables)}>{t('retry')}</Button></Group>
          </Stack>
        </Paper>}
        {pending ? <Text fz="md" role="status">{t(sync.variables === 'disable' ? 'modelsDevDisabling' : 'modelsDevSyncing')}</Text> : sync.isSuccess && <Text fz="md" role="status">{t(sync.variables === 'disable' ? 'modelsDevDisableSuccess' : sync.data?.status === 'not_modified' ? 'catalogRefreshNotModified' : 'modelsDevSyncSuccess')}</Text>}
        <Group gap="sm" wrap="wrap" aria-busy={pending}>
          <Button mih={44} style={instantActionStyle} leftSection={!source ? undefined : source.enabled ? <RefreshCw size={16} aria-hidden="true" /> : <Power size={16} aria-hidden="true" />} loading={pending && sync.variables !== 'disable'} disabled={pending} onClick={() => sync.mutate(primaryAction)}>{t(primaryLabel)}</Button>
          {source && <>
            {source.enabled && <Button mih={44} style={instantActionStyle} variant="default" leftSection={<Power size={16} aria-hidden="true" />} loading={pending && sync.variables === 'disable'} disabled={pending} onClick={() => sync.mutate('disable')}>{t('modelsDevDisable')}</Button>}
            <Button mih={44} style={instantActionStyle} variant="default" disabled={pending} onClick={() => onEdit(source)}>{t('editSubscription')}</Button>
          </>}
        </Group>
      </Stack>
    </Paper>
  )
}
