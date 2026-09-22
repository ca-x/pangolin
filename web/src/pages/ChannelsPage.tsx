import { ActionIcon, Alert, Badge, Button, Card, Collapse, Group, Modal, Paper, SimpleGrid, Skeleton, Stack, Switch, Tabs, Text, Textarea, TextInput, Title, Tooltip } from '@mantine/core'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { ChevronRight, Plus, Trash2 } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { api, type Document, type Paged } from '../api'
import { EnabledPill, InlineQueryError, SelectField, SkeletonRows } from '../components'
import { UNMEASURED, useErrorCodeLabel } from '../observability'
import { ProviderIcon } from '../ProviderIcon'
import { CHANNEL_PROVIDERS } from '../providers'
import { projectOperationPath, useProject } from '../project'
import { BulkToggle, PageHeader, QueryError, ResourcePage, displayValue, formatDate } from './shared'

const CHANNEL_TABS = ['channels', 'credentials', 'channelPolicies', 'probes', 'quotas', 'presets'] as const

/**
 * One row of `channel_health_state` or `credential_health_state`, exactly as the
 * operations projection returns it. Every field is optional here because the
 * console must render what was recorded and nothing else.
 */
type HealthRow = { id: string; provider_id?: string; credential_id?: string; consecutive_failures?: number; disabled_until?: number | null; backoff_until?: number | null; reason?: string | null; updated_at?: number }

export default function ChannelsPage() {
  const { t } = useTranslation()
  const kindLabel = useProviderKindLabel()
  return (
    <Tabs keepMounted={false} defaultValue="channels">
      <Tabs.List mb="lg">
        {CHANNEL_TABS.map((value) => (
          <Tabs.Tab key={value} value={value}>{t(value)}</Tabs.Tab>
        ))}
      </Tabs.List>
      <Tabs.Panel value="channels">
        <ResourcePage resource="channels" title={t('channels')} description={t('channelsDescription')} empty={t('channelEmpty')} createLabel={t('addChannel')} selectable="channels" bulkDelete notice={<HealthNotice kind="channel" />} columns={[{ key: 'name', label: t('name'), render: (value, row) => <span className="provider-cell"><ProviderIcon logoKey={String((row.settings as Record<string, unknown>)?.logo_key || '')} name={String(value)} /><strong>{String(value)}</strong></span> }, { key: 'kind', label: t('providerType'), render: (value) => kindLabel(value) }, { key: 'base_url', label: t('baseUrl'), mono: true }, { key: 'health', label: t('channelHealth'), render: (_value, row) => <HealthCell kind="channel" id={String(row.id)} /> }, { key: 'enabled', label: t('status'), render: (value) => <EnabledPill enabled={value} /> }]} rowActions={(row) => <><ProbeRowAction id={String(row.id)} name={displayValue(row.name || row.id)} /><ChannelLifecycleActions row={row} /></>} fields={[{ key: 'name', label: t('name'), required: true }, { key: 'kind', label: t('providerType'), kind: 'provider', required: true, baseUrlKey: 'base_url', options: providerOptions(t) }, { key: 'base_url', label: t('baseUrl'), required: true, defaultValue: CHANNEL_PROVIDERS[0].defaultBaseUrl ?? '' }, { key: 'settings', label: t('advancedSettings'), kind: 'json', defaultValue: { version: 1, tags: [], limits: {}, circuit: { failures: 5, window_ms: 60000, recovery_ms: 30000 }, quota: { path: '/api/v1/key' } } }, { key: 'enabled', label: t('status'), kind: 'checkbox' }]} />
        <BulkToggle />
      </Tabs.Panel>
      <Tabs.Panel value="credentials"><CredentialsPanel /></Tabs.Panel>
      <Tabs.Panel value="channelPolicies"><ChannelSettingsPanel /></Tabs.Panel>
      <Tabs.Panel value="probes">
        <Diagnostics />
        <ProbesPanel />
      </Tabs.Panel>
      <Tabs.Panel value="quotas">
        <Diagnostics quota />
        <ResourcePage resource="quotas" title={t('quotas')} description={t('quotasDescription')} empty={t('quotaEmpty')} immutable columns={[{ key: 'provider_id', label: t('channel'), mono: true }, { key: 'remaining_micros', label: t('remaining'), render: (value, row) => (row.quota as Document)?.measured === false ? '—' : displayValue(value) }, { key: 'period', label: t('quotaPeriod'), render: (_value, row) => displayValue((row.quota as Document)?.period) }, { key: 'period_end', label: t('periodEnd'), render: formatDate }, { key: 'source_url', label: t('quotaUrl'), render: (_value, row) => displayValue((row.quota as Document)?.source_url) }, { key: 'collected_at', label: t('time'), render: formatDate }]} />
      </Tabs.Panel>
      <Tabs.Panel value="presets"><PresetsPanel /></Tabs.Panel>
    </Tabs>
  )
}

/** The picker's options: the adapter kind is the submitted value, the visible
 *  name is localized and the icon comes from the bundled catalog icon map. */
function providerOptions(label: (key: string) => string) {
  return CHANNEL_PROVIDERS.map((provider) => ({ value: provider.kind, label: label(provider.labelKey), logoKey: provider.logoKey, defaultBaseUrl: provider.defaultBaseUrl }))
}

/**
 * The adapter kind a channel stores, as the name the picker offered. The record
 * system keeps the enum (`openai_compatible`) and the console keeps the copy, so
 * a table of Chinese labels no longer carries one English enum in it. A kind this
 * build does not know — a channel from a newer backend — is shown verbatim.
 */
function useProviderKindLabel() {
  const { t } = useTranslation()
  return (kind: unknown) => {
    const provider = CHANNEL_PROVIDERS.find((entry) => entry.kind === kind)
    return provider ? t(provider.labelKey) : displayValue(kind)
  }
}

/** Credential types as the record system stores them; anything else is data. */
const CREDENTIAL_TYPE_KEYS: Record<string, string> = { api_key: 'credentialTypeApiKey' }

function useCredentialTypeLabel() {
  const { t } = useTranslation()
  return (type: unknown) => {
    const key = CREDENTIAL_TYPE_KEYS[String(type)]
    return key ? t(key) : displayValue(type)
  }
}

/**
 * The recorded probe attempts. The projection sends a channel id, an integer
 * outcome, epoch seconds and `0` for a probe that never reached the upstream; none
 * of those is a name or a measurement, so each is resolved before it is shown.
 */
function ProbesPanel() {
  const { t } = useTranslation()
  const errorLabel = useErrorCodeLabel()
  const { options: channelOptions } = useChannelOptions()
  const channelName = (value: unknown) => channelOptions.find((option) => option.value === String(value))?.label ?? displayValue(value)
  const measured = (value: unknown) => typeof value === 'number' && value > 0 ? String(value) : '—'
  const outcome = (value: unknown) => value === 1 || value === true
  return <ResourcePage resource="probes" title={t('probes')} description={t('probesDescription')} empty={t('probeEmpty')} immutable columns={[{ key: 'provider_id', label: t('channel'), render: (value) => channelName(value) }, { key: 'success', label: t('status'), render: (value) => <Badge variant="light" color={outcome(value) ? 'green' : 'red'}>{t(outcome(value) ? 'statusSucceeded' : 'statusFailed')}</Badge> }, { key: 'status_code', label: t('statusCode'), render: (value) => measured(value) }, { key: 'latency_ms', label: t('latency'), render: (value) => measured(value) }, { key: 'probed_at', label: t('time'), render: formatDate }, { key: 'error', label: t('probeError'), render: (value) => value ? errorLabel(String(value)) : '—' }]} />
}

function useChannelOptions() {
  const { project } = useProject()
  const query = useQuery({ queryKey: ['channel-options', project.id], queryFn: () => api<Paged<Document>>(projectOperationPath(project.id, 'channels') + '?limit=500') })
  return { query, options: (query.data?.data || []).map((row) => ({ value: String(row.id), label: String(row.name) })) }
}

function CredentialRecoveryAction({ row }: { row: Document }) {
  const { t } = useTranslation()
  const { project } = useProject()
  const client = useQueryClient()
  const [open, setOpen] = useState(false)
  const [secret, setSecret] = useState('')
  const [token, setToken] = useState('')
  const issue = useMutation({ mutationFn: () => api<{ token: string }>(`/api/admin/v1/projects/${project.id}/credentials/${row.id}/recovery-token`, { method: 'POST', body: JSON.stringify({ expires_seconds: 600 }) }), onSuccess: (result) => { setToken(result.token); setOpen(true) }, onError: (error: Error) => toast.error(error.message) })
  const recover = useMutation({ mutationFn: () => api(`/api/admin/v1/projects/${project.id}/credentials/${row.id}/recover`, { method: 'POST', body: JSON.stringify({ token, secret }) }), onSuccess: () => { setOpen(false); setToken(''); setSecret(''); toast.success(t('credentialRecovered')); void client.invalidateQueries({ queryKey: ['resource', project.id, 'credentials'] }) }, onError: (error: Error) => toast.error(error.message) })
  return <>
    <Button variant="subtle" color="red" size="compact-sm" loading={issue.isPending} onClick={() => issue.mutate()}>{t('replaceCredential')}</Button>
    <Modal opened={open} onClose={() => !recover.isPending && setOpen(false)} title={t('replaceCredential')} closeButtonProps={{ 'aria-label': t('close') }}>
      <Stack gap="md"><Alert color="red">{t('credentialUnrecoverableHint')}</Alert><TextInput type="password" label={t('replacementSecret')} value={secret} onChange={(event) => setSecret(event.currentTarget.value)} required /><Group justify="flex-end"><Button variant="default" onClick={() => setOpen(false)}>{t('cancel')}</Button><Button disabled={!secret} loading={recover.isPending} onClick={() => recover.mutate()}>{t('replaceCredential')}</Button></Group></Stack>
    </Modal>
  </>
}

function ChannelLifecycleActions({ row }: { row: Document }) {
  const { t } = useTranslation()
  const { project } = useProject()
  const client = useQueryClient()
  const { options } = useChannelOptions()
  const [mode, setMode] = useState<'clone' | 'merge' | null>(null)
  const [name, setName] = useState(`${String(row.name)} copy`)
  const [target, setTarget] = useState('')
  const [preview, setPreview] = useState<{ dependencies?: Array<{ models: number; credentials: number }>; collisions?: string[] } | null>(null)
  const mutation = useMutation({ mutationFn: async () => {
    const ids = mode === 'merge' ? [String(row.id), target] : [String(row.id)]
    const result = await api<typeof preview>(projectOperationPath(project.id, 'channel-preview'), { method: 'POST', body: JSON.stringify({ action: mode, ids }) })
    setPreview(result)
    if (result?.collisions?.length) throw new Error(t('mergeModelCollision', { names: result.collisions.join(', ') }))
    return api(projectOperationPath(project.id, mode === 'merge' ? 'channel-merge' : 'channel-clone'), { method: 'POST', body: JSON.stringify(mode === 'merge' ? { source_id: row.id, target_id: target } : { source_id: row.id, name }) })
  }, onSuccess: () => { toast.success(t('saved')); setMode(null); setPreview(null); void client.invalidateQueries({ queryKey: ['resource', project.id] }) }, onError: (error: Error) => toast.error(error.message) })
  const dependencies = preview?.dependencies?.[0]
  return <>
    <Button variant="subtle" size="compact-sm" onClick={() => { setPreview(null); setMode('clone') }}>{t('cloneChannel')}</Button>
    <Button variant="subtle" size="compact-sm" onClick={() => { setPreview(null); setMode('merge') }}>{t('mergeChannel')}</Button>
    <Modal opened={mode !== null} onClose={() => !mutation.isPending && setMode(null)} title={t(mode === 'merge' ? 'mergeChannel' : 'cloneChannel')} closeButtonProps={{ 'aria-label': t('close') }}>
      <Stack gap="md">
        {mode === 'clone' ? <TextInput label={t('name')} value={name} onChange={(event) => setName(event.currentTarget.value)} required /> : <SelectField label={t('targetChannel')} value={target} onValueChange={setTarget} options={options.filter((option) => option.value !== String(row.id))} />}
        {dependencies && <Text size="sm" c="dimmed">{t('channelDependencyPreview', dependencies)}</Text>}
        <Text size="sm" c="dimmed">{t('channelSecretsNotCopied')}</Text>
        <Group justify="flex-end"><Button variant="default" onClick={() => setMode(null)}>{t('cancel')}</Button><Button disabled={mode === 'merge' ? !target : !name.trim()} loading={mutation.isPending} onClick={() => mutation.mutate()}>{t(mode === 'merge' ? 'mergeChannel' : 'cloneChannel')}</Button></Group>
      </Stack>
    </Modal>
  </>
}

/**
 * The recorded health of every channel and credential in the project. The
 * gateway serves both projections already; without them the console could only
 * say "disabled" and an operator had to guess why. A failed read is reported as
 * a failure — never as healthy, and never as an invented zero.
 *
 * The read is subscribed by each cell (and by the notice) rather than by the
 * page: the projection lands after the table's own rows, and a page-level
 * subscription would re-render — and so remount — every row's controls each time
 * it settled. Cells sharing this key still cost one request.
 */
const healthQuery = (project: string, kind: 'channel' | 'credential') => ({
  queryKey: [kind === 'channel' ? 'channel-health' : 'credential-health', project],
  queryFn: () => api<Paged<HealthRow>>(projectOperationPath(project, kind === 'channel' ? 'health' : 'credential-health') + '?limit=500'),
})
const healthKey = (kind: 'channel' | 'credential') => kind === 'channel' ? 'provider_id' : 'credential_id'

/** One row's health, from the projection's own record of that channel or credential. */
function HealthCell({ kind, id }: { kind: 'channel' | 'credential'; id: string }) {
  const { project } = useProject()
  const query = useQuery(healthQuery(project.id, kind))
  // Loading is its own state: `—` while the read is in flight would read as "no
  // record" before the record arrives.
  if (query.isLoading) return <Skeleton height={18} width={72} radius="sm" />
  // The projection names a channel's key `provider_id` and a credential's
  // `credential_id`; older responses of the credential relation only carried `id`,
  // which left the column blank for every credential.
  const row = (query.data?.data || []).find((item) => String(item[healthKey(kind)] ?? item.id) === id)
  return <HealthPill health={row} kind={kind} />
}

/** The banner a page shows when the health projection itself cannot be read. */
function HealthNotice({ kind }: { kind: 'channel' | 'credential' }) {
  const { t } = useTranslation()
  const { project } = useProject()
  const query = useQuery(healthQuery(project.id, kind))
  if (!query.isError) return null
  return <div className="resource-notice"><InlineQueryError message={kind === 'channel' ? t('channelHealthUnavailable') : t('credentialHealthUnavailable')} onRetry={() => void query.refetch()} /></div>
}

/**
 * What the gateway recorded about one channel or credential: an auto-disable
 * window, a short backoff, a run of consecutive failures, or nothing at all.
 * `—` when the projection has no row — an absent record is unknown, not healthy.
 */
function HealthPill({ health, kind = 'channel' }: { health?: HealthRow; kind?: 'channel' | 'credential' }) {
  const { t } = useTranslation()
  if (!health) return <>—</>
  const now = Math.floor(Date.now() / 1000)
  const disabledUntil = typeof health.disabled_until === 'number' && health.disabled_until > now ? health.disabled_until : null
  const backoffUntil = kind === 'channel' && typeof health.backoff_until === 'number' && health.backoff_until > now ? health.backoff_until : null
  const failures = typeof health.consecutive_failures === 'number' ? health.consecutive_failures : null
  if (!disabledUntil && !backoffUntil && failures == null) return <>—</>
  const label = disabledUntil ? t('healthDisabledUntil', { time: formatDate(disabledUntil) })
    : backoffUntil ? t('healthBackoffUntil', { time: formatDate(backoffUntil) })
      : failures ? t('healthFailures', { count: failures })
        : t('healthHealthy')
  const color = disabledUntil ? 'red' : backoffUntil || failures ? 'yellow' : 'teal'
  const pill = <Badge variant="light" color={color} radius="sm">{label}</Badge>
  // The recorded reason is the gateway's own error code: data, shown verbatim.
  return health.reason ? <Tooltip label={t('healthLastError', { reason: health.reason })}>{pill}</Tooltip> : pill
}

function CredentialsPanel() {
  const { t } = useTranslation()
  const credentialType = useCredentialTypeLabel()
  const { options, query } = useChannelOptions()
  // A picker fed by a failed lookup offers an empty choice that reads as a real
  // one, so the field carries the failure and its retry instead.
  const optionsError = query.isError ? t('optionsUnavailable') : undefined
  const retryOptions = () => void query.refetch()
  return (
    <>
      <ResourcePage resource="credentials" title={t('credentials')} description={t('credentialsDescription')} empty={t('credentialEmpty')} selectable="credentials" createLabel={t('addCredential')} notice={<HealthNotice kind="credential" />} columns={[{ key: 'provider_name', label: t('channel') }, { key: 'suffix', label: t('suffix'), mono: true }, { key: 'credential_type', label: t('credentialType'), render: (value) => credentialType(value) }, { key: 'state', label: t('credentialState'), render: (value) => <Badge variant="light" color={value === 'unrecoverable' ? 'red' : 'green'}>{t(value === 'unrecoverable' ? 'credentialUnrecoverable' : 'credentialReady')}</Badge> }, { key: 'priority', label: t('priority') }, { key: 'health', label: t('credentialHealth'), render: (_value, row) => <HealthCell kind="credential" id={String(row.id)} /> }, { key: 'enabled', label: t('status'), render: (value) => <EnabledPill enabled={value} /> }]} rowActions={(row) => row.state === 'unrecoverable' ? <CredentialRecoveryAction row={row} /> : null} fields={[{ key: 'provider_id', label: t('channel'), kind: 'select', required: true, options, error: optionsError, onRetry: retryOptions }, { key: 'credential_type', label: t('credentialType'), defaultValue: 'api_key', required: true }, { key: 'secret', label: t('secret'), kind: 'secret', hint: t('secretUpdateHint'), omitWhenBlank: true }, { key: 'priority', label: t('priority'), kind: 'number', defaultValue: 100 }, { key: 'enabled', label: t('status'), kind: 'checkbox' }, { key: 'settings', label: t('advancedSettings'), kind: 'json', defaultValue: { version: 1 } }]} />
      <BulkToggle resource="credentials" />
    </>
  )
}

function ChannelSettingsPanel() {
  const { t } = useTranslation()
  const { options, query } = useChannelOptions()
  return (
    <ResourcePage resource="channel-settings" title={t('channelPolicies')} description={t('channelPoliciesHint')} empty={t('channelPoliciesEmpty')} canDelete={false} columns={[{ key: 'provider_id', label: t('channel'), render: (value) => options.find((option) => option.value === value)?.label || String(value) }, { key: 'retry_statuses', label: t('retryStatuses') }, { key: 'auto_disable_policy', label: t('autoDisable') }]} fields={[{ key: 'provider_id', label: t('channel'), kind: 'select', required: true, options, error: query.isError ? t('optionsUnavailable') : undefined, onRetry: () => void query.refetch() }, { key: 'endpoint_mappings', label: t('endpointMappings'), kind: 'json', defaultValue: { version: 1 } }, { key: 'model_rules', label: t('modelRules'), kind: 'json', defaultValue: { version: 1 }, render: (input) => <ModelRulesEditor {...input} /> }, { key: 'parameter_overrides', label: t('parameterOverrides'), kind: 'json', defaultValue: { version: 1 } }, { key: 'retry_statuses', label: t('retryStatuses'), kind: 'json', defaultValue: { version: 1, statuses: [408, 409, 429, 500, 502, 503, 504] } }, { key: 'auto_disable_policy', label: t('autoDisable'), kind: 'json', defaultValue: { version: 1, enabled: false } }]} />
  )
}

type MappingRow = { id: number; source: string; target: string }

function modelRules(value: unknown): Record<string, unknown> {
  if (value && typeof value === 'object' && !Array.isArray(value)) return value as Record<string, unknown>
  if (typeof value === 'string') {
    try {
      const parsed: unknown = JSON.parse(value)
      if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) return parsed as Record<string, unknown>
    } catch { /* A malformed stored document falls back to the repairable versioned shape. */ }
  }
  return { version: 1 }
}

function parseModelRules(value: string): Record<string, unknown> | null {
  try {
    const parsed: unknown = JSON.parse(value)
    return parsed && typeof parsed === 'object' && !Array.isArray(parsed) ? parsed as Record<string, unknown> : null
  } catch {
    return null
  }
}

function mappingRows(value: Record<string, unknown>): MappingRow[] {
  const mappings = value.mappings
  if (!mappings || typeof mappings !== 'object' || Array.isArray(mappings)) return []
  return Object.entries(mappings).map(([source, target], index) => ({ id: index, source, target: String(target) }))
}

/** A channel mapping remains one model-rules document on the wire. Structured
 * controls edit the common cases; the bounded JSON field remains available for
 * the older transformation and request-shape rules that share the document. */
function ModelRulesEditor({ name, label, value, setValid }: { name: string; label: string; value: unknown; setValid: (valid: boolean) => void }) {
  const { t } = useTranslation()
  const initial = modelRules(value)
  const [text, setText] = useState(() => JSON.stringify(initial, null, 2))
  const [rows, setRows] = useState<MappingRow[]>(() => mappingRows(initial))
  const [prefixText, setPrefixText] = useState(() => Array.isArray(initial.auto_trim_prefixes) ? initial.auto_trim_prefixes.filter((prefix): prefix is string => typeof prefix === 'string').join(', ') : '')
  const [nextId, setNextId] = useState(rows.length)
  const [advanced, setAdvanced] = useState(false)
  const rules = parseModelRules(text)

  const write = (next: Record<string, unknown>, nextRows = rows) => {
    const mappings = Object.fromEntries(nextRows.filter((row) => row.source && row.target).map((row) => [row.source, row.target]))
    setText(JSON.stringify({ ...next, version: 1, mappings }, null, 2))
  }
  const update = (key: string, next: unknown) => { if (rules) write({ ...rules, [key]: next }) }
  const updateRows = (next: MappingRow[]) => {
    setRows(next)
    if (rules) write(rules, next)
  }
  const duplicateSources = rows.some((row, index) => row.source && rows.findIndex((item) => item.source === row.source) !== index)
  useEffect(() => setValid(Boolean(rules) && !duplicateSources), [duplicateSources, rules, setValid])
  return <Stack gap="sm">
    <Stack gap={2}>
      <Text fw={600} size="sm">{label}</Text>
      <Text size="xs" c="dimmed">{t('modelRulesHint')}</Text>
    </Stack>
    <TextInput
      label={t('autoTrimPrefixes')}
      description={t('autoTrimPrefixesHint')}
      placeholder={t('autoTrimPrefixesPlaceholder')}
      value={prefixText}
      disabled={!rules}
      onChange={(event) => {
        const next = event.currentTarget.value
        setPrefixText(next)
        update('auto_trim_prefixes', next.split(',').map((prefix) => prefix.trim()).filter(Boolean))
      }}
    />
    <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="sm">
      <Switch checked={rules?.hide_original === true} disabled={!rules} label={t('hideOriginalModels')} description={t('hideOriginalModelsHint')} onChange={(event) => update('hide_original', event.currentTarget.checked)} />
      <Switch checked={rules?.hide_mapped === true} disabled={!rules} label={t('hideMappedModels')} description={t('hideMappedModelsHint')} onChange={(event) => update('hide_mapped', event.currentTarget.checked)} />
    </SimpleGrid>
    <Stack gap="xs">
      <Group justify="space-between" align="center">
        <Text fw={600} size="sm">{t('modelMappings')}</Text>
        <Button variant="default" size="compact-sm" leftSection={<Plus size={15} />} disabled={!rules} onClick={() => { setRows((current) => [...current, { id: nextId, source: '', target: '' }]); setNextId((current) => current + 1) }}>{t('addMapping')}</Button>
      </Group>
      {rows.length === 0 && <Text size="sm" c="dimmed">{t('modelMappingsEmpty')}</Text>}
      {rows.map((row, index) => <Group key={row.id} gap="xs" align="flex-start" wrap="nowrap">
        <TextInput aria-label={`${t('mappingSource')} ${index + 1}`} placeholder={t('mappingSource')} value={row.source} required ref={(input) => input?.setCustomValidity(row.source && rows.some((item) => item.id !== row.id && item.source === row.source) ? t('duplicateMappingSource') : '')} onChange={(event) => updateRows(rows.map((item) => item.id === row.id ? { ...item, source: event.currentTarget.value } : item))} style={{ flex: 1, minWidth: 0 }} />
        <TextInput aria-label={`${t('mappingTarget')} ${index + 1}`} placeholder={t('mappingTarget')} value={row.target} required onChange={(event) => updateRows(rows.map((item) => item.id === row.id ? { ...item, target: event.currentTarget.value } : item))} style={{ flex: 1, minWidth: 0 }} />
        <Tooltip label={t('removeMapping')}><ActionIcon size={40} variant="subtle" color="red" aria-label={`${t('removeMapping')} ${index + 1}`} onClick={() => updateRows(rows.filter((item) => item.id !== row.id))}><Trash2 size={16} /></ActionIcon></Tooltip>
      </Group>)}
      {duplicateSources && <Text size="xs" c="red" role="alert">{t('duplicateMappingSource')}</Text>}
    </Stack>
    <Button variant="subtle" size="compact-sm" justify="flex-start" leftSection={<ChevronRight size={15} style={{ transform: advanced ? 'rotate(90deg)' : undefined, transition: 'transform 150ms' }} />} onClick={() => setAdvanced((open) => !open)} aria-expanded={advanced}>{t('advancedModelRules')}</Button>
    <Collapse expanded={advanced} keepMounted>
      <div inert={!advanced}>
        <Textarea name={name} aria-label={t('advancedModelRules')} value={text} rows={9} error={!rules ? t('invalidJson') : duplicateSources ? t('duplicateMappingSource') : undefined} onChange={(event) => {
          const next = event.currentTarget.value
          setText(next)
          const parsed = parseModelRules(next)
          if (parsed) {
            setRows(mappingRows(parsed))
            setPrefixText(Array.isArray(parsed.auto_trim_prefixes) ? parsed.auto_trim_prefixes.filter((prefix): prefix is string => typeof prefix === 'string').join(', ') : '')
          }
        }} />
      </div>
    </Collapse>
  </Stack>
}

type ProbeRow = { id?: string; provider_id?: string; model?: string | null; success?: number | null; status_code?: number | null; latency_ms?: number | null; probed_at?: number | null; error?: string | null }

/**
 * Probe one channel from its own row. The job is the same durable one the Probes
 * tab enqueues, but the outcome is reported where the operator asked for it: a
 * dialog that watches the probe records and shows the first one newer than the
 * moment the job was queued, with the model the backend chose — the console does
 * not pick it, and saying otherwise would be an invention.
 */
function ProbeRowAction({ id, name }: { id: string; name: string }) {
  const { t } = useTranslation()
  const { project } = useProject()
  const [open, setOpen] = useState(false)
  const [baseline, setBaseline] = useState<number | null>(null)
  const [queued, setQueued] = useState(false)
  const query = useQuery({
    queryKey: ['probe-row', project.id, id],
    queryFn: () => api<Paged<ProbeRow>>(projectOperationPath(project.id, 'probes') + '?limit=50'),
    enabled: open,
    // A queued job is expected to land shortly; polling stops once it has.
    refetchInterval: open && queued ? 2000 : false,
  })
  const rows = query.data?.data || []
  const newest = rows.reduce((value, row) => Math.max(value, Number(row.probed_at ?? 0)), 0)
  const run = useMutation({
    mutationFn: () => api(projectOperationPath(project.id, 'probe'), { method: 'POST', body: JSON.stringify({ provider_id: id }) }),
    // The baseline is the newest record *before* this run, so the result shown is
    // this probe's and not an older one; the read is repeated straight away because
    // a fast job would otherwise wait for the polling interval.
    onSuccess: () => { setBaseline(newest); setQueued(true); void query.refetch() },
    onError: (error: Error) => toast.error(error.message),
  })
  const result = rows
    .filter((row) => String(row.provider_id) === id && (baseline == null || Number(row.probed_at ?? 0) > baseline))
    .sort((left, right) => Number(right.probed_at ?? 0) - Number(left.probed_at ?? 0))[0]
  const outcome = result ? result.success === 1 : null
  const openDialog = () => { setOpen(true); setBaseline(null); setQueued(false) }
  const closeDialog = () => setOpen(false)
  return <>
    <Button variant="subtle" size="compact-sm" aria-label={`${t('test')} ${name}`} onClick={openDialog}>{t('test')}</Button>
    <Modal opened={open} onClose={closeDialog} title={`${t('test')} ${name}`} closeButtonProps={{ 'aria-label': t('close') }}>
      <Stack gap="md">
        <Group gap="sm" align="end" wrap="wrap">
          <Button onClick={() => run.mutate()} loading={run.isPending}>{t('runProbe')}</Button>
          <Text size="xs" c="dimmed">{t('probeChoosesModel')}</Text>
        </Group>
        {run.isError && <InlineQueryError message={run.error.message} onRetry={() => run.mutate()} />}
        {query.isError ? <InlineQueryError message={t('probeResultsUnavailable')} onRetry={() => void query.refetch()} />
          : !queued ? null
            : !result ? <Text size="sm" c="dimmed">{t('probeWaiting')}</Text>
              : <Stack gap={4}>
                <Badge variant="light" color={outcome ? 'green' : 'red'}>{t(outcome ? 'statusSucceeded' : 'statusFailed')}</Badge>
                <Text size="sm">{`${t('model')}: ${result.model || UNMEASURED}`}</Text>
                <Text size="sm">{`${t('latency')}: ${result.latency_ms == null || result.latency_ms <= 0 ? UNMEASURED : `${result.latency_ms} ms`}`}</Text>
                <Text size="sm">{`${t('statusCode')}: ${result.status_code == null || result.status_code <= 0 ? UNMEASURED : result.status_code}`}</Text>
                {result.error && <Text size="sm" className="error-text">{`${t('probeError')}: ${result.error}`}</Text>}
              </Stack>}
      </Stack>
    </Modal>
  </>
}

function Diagnostics({ quota = false }: { quota?: boolean }) {
  const { t } = useTranslation()
  const { project } = useProject()
  const { options, query } = useChannelOptions()
  const [selected, setSelected] = useState('')
  const run = useMutation({ mutationFn: (provider_id: string) => api(projectOperationPath(project.id, quota ? 'quota' : 'probe'), { method: 'POST', body: JSON.stringify({ provider_id }) }), onSuccess: () => toast.success(t('jobQueued')), onError: (error: Error) => toast.error(error.message) })
  const current = selected || options[0]?.value || ''
  return (
    <Paper withBorder p="md" mb="md">
      <form onSubmit={(event) => { event.preventDefault(); run.mutate(current) }}>
        <Group gap="md" align="end" wrap="wrap">
          {/* The probe targets a channel, so a channel list that could not be read
              is reported here rather than as an empty picker. */}
          {query.isError
            ? <InlineQueryError message={t('optionsUnavailable')} onRetry={() => void query.refetch()} />
            : <SelectField label={t('channel')} value={current} onValueChange={setSelected} options={options} />}
          <Button type="submit" disabled={!current || query.isError}>{quota ? t('collectQuota') : t('runProbe')}</Button>
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
