import { ActionIcon, Alert, Badge, Button, Card, Code, Collapse, Group, Modal, Paper, Select, SimpleGrid, Skeleton, Stack, Switch, Tabs, Text, Textarea, TextInput, Title, Tooltip } from '@mantine/core'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { ChevronRight, Plus, Trash2 } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
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
        <ResourcePage resource="channels" title={t('channels')} description={t('channelsDescription')} empty={t('channelEmpty')} createLabel={t('addChannel')} selectable="channels" bulkDelete notice={<HealthNotice kind="channel" />} columns={[{ key: 'name', label: t('name'), render: (value, row) => <span className="provider-cell"><ProviderIcon logoKey={String((row.settings as Record<string, unknown>)?.logo_key || '')} name={String(value)} /><strong>{String(value)}</strong></span> }, { key: 'kind', label: t('providerType'), render: (value) => kindLabel(value) }, { key: 'base_url', label: t('baseUrl'), mono: true }, { key: 'health', label: t('channelHealth'), render: (_value, row) => <HealthCell kind="channel" id={String(row.id)} /> }, { key: 'enabled', label: t('status'), render: (value) => <EnabledPill enabled={value} /> }]} rowActions={(row) => <><ChannelSyncAction id={String(row.id)} name={displayValue(row.name || row.id)} /><ProbeRowAction id={String(row.id)} name={displayValue(row.name || row.id)} /><ChannelLifecycleActions row={row} /></>} fields={[{ key: 'name', label: t('name'), required: true }, { key: 'kind', label: t('providerType'), kind: 'provider', required: true, baseUrlKey: 'base_url', options: providerOptions(t) }, { key: 'base_url', label: t('baseUrl'), required: true, defaultValue: CHANNEL_PROVIDERS[0].defaultBaseUrl ?? '' }, { key: 'settings', label: t('advancedSettings'), kind: 'json', defaultValue: { version: 1, tags: [], limits: {}, circuit: { failures: 5, window_ms: 60000, recovery_ms: 30000 }, quota: { path: '/api/v1/key' } } }, { key: 'enabled', label: t('status'), kind: 'checkbox' }]} />
        <BulkProbe />
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
const CREDENTIAL_TYPE_KEYS: Record<string, string> = {
  api_key: 'credentialTypeApiKey',
  oauth_codex: 'oauthFlow_codex',
  oauth_xai: 'oauthFlow_xai',
  oauth_claude_code: 'oauthFlow_claude_code',
  oauth_antigravity: 'oauthFlow_antigravity',
  oauth_github_copilot: 'oauthFlow_github_copilot',
}

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
  const rate = (value: unknown, row: Document) => {
    if (typeof row.output_tokens !== 'number' || typeof row.latency_ms !== 'number' || typeof row.ttft_ms !== 'number') return UNMEASURED
    return row.output_tokens >= 0 && row.latency_ms > row.ttft_ms ? (row.output_tokens * 1000 / (row.latency_ms - row.ttft_ms)).toFixed(1) : UNMEASURED
  }
  return <ResourcePage resource="probes" title={t('probes')} description={t('probesDescription')} empty={t('probeEmpty')} immutable columns={[{ key: 'provider_id', label: t('channel'), render: (value) => channelName(value) }, { key: 'model', label: t('model'), render: displayValue }, { key: 'success', label: t('status'), render: (value) => <Badge variant="light" color={outcome(value) ? 'green' : 'red'}>{t(outcome(value) ? 'statusSucceeded' : 'statusFailed')}</Badge> }, { key: 'status_code', label: t('statusCode'), render: (value) => measured(value) }, { key: 'latency_ms', label: t('latency'), render: (value) => measured(value) }, { key: 'ttft_ms', label: t('ttft'), render: (value) => measured(value) }, { key: 'tokens_per_second', label: t('tokensPerSecond'), render: rate }, { key: 'probed_at', label: t('time'), render: formatDate }, { key: 'error', label: t('probeError'), render: (value) => value ? errorLabel(String(value)) : '—' }]} />
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

type ProbeModel = { id: string; provider_id?: string; public_name?: string; upstream_name?: string; enabled?: boolean; lifecycle?: string }
const modelsQuery = (project: string) => ({
  queryKey: ['probe-models', project],
  queryFn: () => api<Paged<ProbeModel>>(projectOperationPath(project, 'models') + '?limit=500'),
})
function useProbeModels() {
  const { project } = useProject()
  return useQuery(modelsQuery(project.id))
}

type ProbeRow = { id?: string; provider_id?: string; model?: string | null; success?: number | boolean | null; status_code?: number | null; latency_ms?: number | null; ttft_ms?: number | null; output_tokens?: number | null; probed_at?: number | null; error?: string | null }
const probesQuery = (project: string) => ({
  queryKey: ['probe-history', project],
  queryFn: () => api<Paged<ProbeRow>>(projectOperationPath(project, 'probes') + '?limit=500'),
})
function useProbeHistory(polling = false) {
  const { project } = useProject()
  return useQuery({ ...probesQuery(project.id), refetchInterval: polling ? 2000 : false })
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
      <OAuthCredentialPanel options={options} optionsError={optionsError} retryOptions={retryOptions} />
      <ResourcePage resource="credentials" title={t('credentials')} description={t('credentialsDescription')} empty={t('credentialEmpty')} selectable="credentials" createLabel={t('addCredential')} notice={<HealthNotice kind="credential" />} columns={[{ key: 'provider_name', label: t('channel') }, { key: 'suffix', label: t('suffix'), mono: true }, { key: 'credential_type', label: t('credentialType'), render: (value) => credentialType(value) }, { key: 'state', label: t('credentialState'), render: (value) => <Badge variant="light" color={value === 'unrecoverable' ? 'red' : 'green'}>{t(value === 'unrecoverable' ? 'credentialUnrecoverable' : 'credentialReady')}</Badge> }, { key: 'priority', label: t('priority') }, { key: 'health', label: t('credentialHealth'), render: (_value, row) => <HealthCell kind="credential" id={String(row.id)} /> }, { key: 'enabled', label: t('status'), render: (value) => <EnabledPill enabled={value} /> }]} rowActions={(row) => row.state === 'unrecoverable' ? <CredentialRecoveryAction row={row} /> : null} fields={[{ key: 'provider_id', label: t('channel'), kind: 'select', required: true, options, error: optionsError, onRetry: retryOptions }, { key: 'credential_type', label: t('credentialType'), defaultValue: 'api_key', required: true }, { key: 'secret', label: t('secret'), kind: 'secret', hint: t('secretUpdateHint'), omitWhenBlank: true }, { key: 'priority', label: t('priority'), kind: 'number', defaultValue: 100 }, { key: 'enabled', label: t('status'), kind: 'checkbox' }, { key: 'settings', label: t('advancedSettings'), kind: 'json', defaultValue: { version: 1 } }]} />
      <BulkToggle resource="credentials" />
    </>
  )
}

function ChannelSyncAction({ id, name }: { id: string; name: string }) {
  const { t } = useTranslation()
  const { project } = useProject()
  const client = useQueryClient()
  const sync = useMutation({
    mutationFn: () => api(projectOperationPath(project.id, 'model-sync'), { method: 'POST', body: JSON.stringify({ provider_id: id }) }),
    onSuccess: () => { toast.success(t('modelSyncQueued')); void client.invalidateQueries({ queryKey: ['resource', project.id, 'channel-settings'] }) },
    onError: (error: Error) => toast.error(error.message),
  })
  return <Button variant="subtle" size="compact-sm" aria-label={`${t('syncModels')} ${name}`} loading={sync.isPending} onClick={() => sync.mutate()}>{t('syncModels')}</Button>
}

type OAuthStart = { state: string; authorization_url?: string; verification_uri?: string; user_code?: string; expires_in?: number; interval?: number }
const OAUTH_FLOWS = ['codex', 'xai', 'claude_code', 'antigravity', 'github_copilot'] as const

function OAuthCredentialPanel({ options, optionsError, retryOptions }: { options: { value: string; label: string }[]; optionsError?: string; retryOptions: () => void }) {
  const { t } = useTranslation()
  const { project } = useProject()
  const client = useQueryClient()
  const [provider, setProvider] = useState('')
  const [flow, setFlow] = useState<(typeof OAUTH_FLOWS)[number]>('codex')
  const [clientId, setClientId] = useState('')
  const [redirectUri, setRedirectUri] = useState(`${window.location.origin}/channels`)
  const [code, setCode] = useState('')
  const [started, setStarted] = useState<OAuthStart | null>(null)
  const selectedProvider = provider || options[0]?.value || ''
  const path = (action: 'start' | 'complete') => `/api/admin/v1/projects/${project.id}/providers/${selectedProvider}/oauth/${flow}/${action}`
  const start = useMutation({
    mutationFn: () => api<OAuthStart>(path('start'), { method: 'POST', body: JSON.stringify({ client_id: clientId, ...(flow === 'github_copilot' ? {} : { redirect_uri: redirectUri }) }) }),
    onSuccess: setStarted,
    onError: (error: Error) => toast.error(error.message),
  })
  const complete = useMutation({
    mutationFn: () => api<{ status: string; retry_after?: number }>(path('complete'), { method: 'POST', body: JSON.stringify({ state: started?.state, ...(flow === 'github_copilot' ? {} : { code }) }) }),
    onSuccess: (result) => {
      if (result.status === 'pending') return toast.info(t('oauthPending', { seconds: result.retry_after }))
      toast.success(t('oauthCredentialSaved'))
      setStarted(null)
      setCode('')
      void client.invalidateQueries({ queryKey: ['resource', project.id, 'credentials'] })
    },
    onError: (error: Error) => toast.error(error.message),
  })
  const resetFlow = (value: string | null) => { setFlow((value || 'codex') as (typeof OAUTH_FLOWS)[number]); setStarted(null); setCode('') }
  return (
    <Paper withBorder p="lg" mb="lg">
      <Stack gap="md">
        <Stack gap={2}>
          <Title order={2}>{t('providerOauth')}</Title>
          <Text size="sm" c="dimmed">{t('providerOauthHint')}</Text>
        </Stack>
        {optionsError ? (
          <Alert variant="light" color="red">
            <Group justify="space-between" align="center">
              <Text size="sm">{t('oauthOptionsUnavailable')}</Text>
              <Button variant="outline" color="red" size="compact-xs" onClick={retryOptions}>{t('oauthRetry')}</Button>
            </Group>
          </Alert>
        ) : (
          <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="md">
            <Select label={t('oauthProvider')} value={selectedProvider} onChange={(value) => { setProvider(value || ''); setStarted(null) }} data={options} searchable required />
            <Select label={t('oauthFlow')} value={flow} onChange={resetFlow} data={OAUTH_FLOWS.map((value) => ({ value, label: t(`oauthFlow_${value}`) }))} required />
            <TextInput label={t('oauthClientId')} value={clientId} onChange={(event) => setClientId(event.currentTarget.value)} required />
            {flow !== 'github_copilot' && <TextInput label={t('oauthRedirectUri')} value={redirectUri} onChange={(event) => setRedirectUri(event.currentTarget.value)} required />}
          </SimpleGrid>
        )}
        {!started ? <Button onClick={() => start.mutate()} loading={start.isPending} disabled={!selectedProvider || !clientId || Boolean(optionsError)}>{t('oauthStart')}</Button> : (
          <Alert variant="light" title={t('oauthContinue')}>
            <Stack gap="sm">
              {started.authorization_url && <Button component="a" href={started.authorization_url} target="_blank" rel="noreferrer" variant="default">{t('oauthOpenProvider')}</Button>}
              {started.verification_uri && <Text size="sm">{t('oauthDeviceInstruction')} <Code>{started.user_code}</Code> <a href={started.verification_uri} target="_blank" rel="noreferrer">{started.verification_uri}</a></Text>}
              {flow !== 'github_copilot' && <TextInput label={t('oauthAuthorizationCode')} value={code} onChange={(event) => setCode(event.currentTarget.value)} required />}
              <Group wrap="wrap">
                <Button onClick={() => complete.mutate()} loading={complete.isPending} disabled={flow !== 'github_copilot' && !code}>{flow === 'github_copilot' ? t('oauthCheck') : t('oauthComplete')}</Button>
                <Button variant="subtle" onClick={() => setStarted(null)}>{t('cancel')}</Button>
              </Group>
            </Stack>
          </Alert>
        )}
      </Stack>
    </Paper>
  )
}

function ChannelSettingsPanel() {
  const { t } = useTranslation()
  const { options, query } = useChannelOptions()
  return (
    <ResourcePage resource="channel-settings" title={t('channelPolicies')} description={t('channelPoliciesHint')} empty={t('channelPoliciesEmpty')} canDelete={false} columns={[{ key: 'provider_id', label: t('channel'), render: (value) => options.find((option) => option.value === value)?.label || String(value) }, { key: 'proxy_url', label: t('proxyUrl') }, { key: 'model_synced_at', label: t('lastModelSync'), render: (value, row) => row.model_sync_error ? <Badge variant="light" color="red">{t('modelSyncFailed')}</Badge> : value ? t('modelSyncResult', { count: row.model_sync_count ?? 0, time: formatDate(value) }) : '—' }, { key: 'auto_disable_policy', label: t('autoDisable') }]} fields={[{ key: 'provider_id', label: t('channel'), kind: 'select', required: true, options, error: query.isError ? t('optionsUnavailable') : undefined, onRetry: () => void query.refetch() }, { key: 'proxy_url', label: t('proxyUrl'), hint: t('proxyUrlHint') }, { key: 'proxy_username', label: t('proxyUsername') }, { key: 'proxy_password', label: t('proxyPassword'), hint: t('proxyPasswordHint'), kind: 'secret', omitWhenBlank: true }, { key: 'proxy_reuse_connections', label: t('proxyReuseConnections'), kind: 'checkbox', defaultValue: true }, { key: 'endpoint_mappings', label: t('endpointMappings'), kind: 'json', defaultValue: { version: 1 } }, { key: 'model_rules', label: t('modelRules'), kind: 'json', defaultValue: { version: 1 }, render: (input) => <ModelRulesEditor {...input} /> }, { key: 'parameter_overrides', label: t('parameterOverrides'), kind: 'json', defaultValue: { version: 1 } }, { key: 'retry_statuses', label: t('retryStatuses'), kind: 'json', defaultValue: { version: 1, statuses: [408, 409, 429, 500, 502, 503, 504] } }, { key: 'auto_disable_policy', label: t('autoDisable'), kind: 'json', defaultValue: { version: 1, enabled: false } }]} />
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

const successfulProbe = (row: ProbeRow) => row.success === 1 || row.success === true
const measuredAverage = (rows: ProbeRow[], field: 'latency_ms' | 'ttft_ms') => {
  const values = rows.map((row) => row[field]).filter((value): value is number => typeof value === 'number' && (field === 'ttft_ms' ? value >= 0 : value > 0))
  return values.length ? Math.round(values.reduce((sum, value) => sum + value, 0) / values.length) : null
}
const probeRate = (row: ProbeRow) => {
  if (typeof row.output_tokens !== 'number' || typeof row.latency_ms !== 'number' || typeof row.ttft_ms !== 'number') return null
  return row.output_tokens >= 0 && row.latency_ms > row.ttft_ms ? row.output_tokens * 1000 / (row.latency_ms - row.ttft_ms) : null
}

function ProbeSparkline({ rows, name, failed, retry }: { rows: ProbeRow[]; name: string; failed: boolean; retry: () => void }) {
  const { t } = useTranslation()
  if (failed) return <Button variant="subtle" size="compact-xs" aria-label={t('probeHistoryUnavailableFor', { name })} onClick={retry}>{t('retry')}</Button>
  const history = [...rows].sort((left, right) => Number(left.probed_at || 0) - Number(right.probed_at || 0)).slice(-12)
  if (!history.length) return <Text component="span" size="xs" c="dimmed" aria-label={t('probeSparklineLabel', { name })}>{UNMEASURED}</Text>
  const points = history.map((row, index) => `${history.length === 1 ? 18 : index * 36 / (history.length - 1)},${successfulProbe(row) ? 4 : 16}`).join(' ')
  const rate = Math.round(history.filter(successfulProbe).length / history.length * 100)
  return <Group gap={4} wrap="nowrap" aria-label={t('probeSparklineLabel', { name })}>
    <svg width="38" height="20" viewBox="0 0 38 20" aria-hidden="true"><polyline points={points} fill="none" stroke="var(--accent)" strokeWidth="2" vectorEffect="non-scaling-stroke" /></svg>
    <Text component="span" size="xs" c="dimmed" ff="monospace">{rate}%</Text>
  </Group>
}

function ProbeHistory({ rows }: { rows: ProbeRow[] }) {
  const { t } = useTranslation()
  if (!rows.length) return <Text size="sm" c="dimmed">{t('probeEmpty')}</Text>
  const successes = rows.filter(successfulProbe).length
  const latency = measuredAverage(rows, 'latency_ms')
  const ttft = measuredAverage(rows, 'ttft_ms')
  const rates = rows.map(probeRate).filter((value): value is number => value != null)
  const tokensPerSecond = rates.length ? rates.reduce((sum, value) => sum + value, 0) / rates.length : null
  const facts = [
    [t('successRate'), `${Math.round(successes / rows.length * 100)}%`],
    [t('averageLatency'), latency == null ? UNMEASURED : `${latency} ms`],
    [t('averageTtft'), ttft == null ? UNMEASURED : `${ttft} ms`],
    [t('tokensPerSecond'), tokensPerSecond == null ? UNMEASURED : tokensPerSecond.toFixed(1)],
  ]
  return <Stack gap="sm">
    <Title order={3}>{t('probeHistory')}</Title>
    <SimpleGrid cols={{ base: 2, sm: 4 }} spacing="xs">{facts.map(([label, value]) => <Paper withBorder p="xs" key={label}><Text size="xs" c="dimmed">{label}</Text><Text fw={650} ff="monospace">{value}</Text></Paper>)}</SimpleGrid>
    <Stack gap={4}>{[...rows].sort((left, right) => Number(right.probed_at || 0) - Number(left.probed_at || 0)).slice(0, 20).map((row) => <Group key={row.id || `${row.probed_at}-${row.model}`} justify="space-between" gap="xs" wrap="nowrap">
      <Stack gap={0} style={{ minWidth: 0 }}><Text size="sm" fw={560} truncate>{row.model || UNMEASURED}</Text><Text size="xs" c="dimmed">{formatDate(row.probed_at)}</Text></Stack>
      <Group gap="xs" wrap="nowrap"><Badge variant="light" color={successfulProbe(row) ? 'green' : 'red'}>{t(successfulProbe(row) ? 'statusSucceeded' : 'statusFailed')}</Badge><Text size="xs" ff="monospace">{row.latency_ms == null || row.latency_ms <= 0 ? UNMEASURED : `${row.latency_ms} ms`}</Text></Group>
    </Group>)}</Stack>
  </Stack>
}

function ProbeRowAction({ id, name }: { id: string; name: string }) {
  const { t } = useTranslation()
  const { project } = useProject()
  const [open, setOpen] = useState(false)
  const [baseline, setBaseline] = useState<number | null>(null)
  const [queued, setQueued] = useState(false)
  const [model, setModel] = useState('')
  const query = useProbeHistory(open && queued)
  const models = useProbeModels()
  const availableModels = (models.data?.data || []).filter((item) => item.provider_id === id && item.enabled !== false && item.lifecycle !== 'archived')
  useEffect(() => { if (open && !availableModels.some((item) => item.id === model)) setModel(availableModels[0]?.id || '') }, [open, model, availableModels])
  const rows = (query.data?.data || []).filter((row) => String(row.provider_id) === id)
  const newest = rows.reduce((value, row) => Math.max(value, Number(row.probed_at ?? 0)), 0)
  const run = useMutation({
    mutationFn: () => api(projectOperationPath(project.id, 'probe'), { method: 'POST', body: JSON.stringify({ provider_id: id, model_id: model }) }),
    // The baseline is the newest record *before* this run, so the result shown is
    // this probe's and not an older one; the read is repeated straight away because
    // a fast job would otherwise wait for the polling interval.
    onSuccess: () => { setBaseline(newest); setQueued(true); void query.refetch() },
    onError: (error: Error) => toast.error(error.message),
  })
  const result = rows
    .filter((row) => String(row.provider_id) === id && (baseline == null || Number(row.probed_at ?? 0) > baseline))
    .sort((left, right) => Number(right.probed_at ?? 0) - Number(left.probed_at ?? 0))[0]
  const outcome = result ? successfulProbe(result) : null
  const openDialog = () => { setOpen(true); setBaseline(null); setQueued(false) }
  const closeDialog = () => setOpen(false)
  return <>
    <Stack gap={2} align="flex-end"><ProbeSparkline rows={rows} name={name} failed={query.isError} retry={() => void query.refetch()} /><Button variant="subtle" size="compact-sm" aria-label={`${t('test')} ${name}`} onClick={openDialog}>{t('test')}</Button></Stack>
    <Modal opened={open} onClose={closeDialog} title={`${t('test')} ${name}`} closeButtonProps={{ 'aria-label': t('close') }}>
      <Stack gap="md">
        <Group gap="sm" align="end" wrap="wrap">
          <SelectField label={t('model')} value={model} onValueChange={setModel} options={availableModels.map((item) => ({ value: item.id, label: `${item.public_name || item.id}${item.upstream_name && item.upstream_name !== item.public_name ? ` → ${item.upstream_name}` : ''}` }))} />
          <Button onClick={() => run.mutate()} loading={run.isPending} disabled={!model || models.isError}>{t('runProbe')}</Button>
        </Group>
        {models.isLoading && <Text size="sm" c="dimmed">{t('loading')}</Text>}
        {!models.isLoading && !models.isError && availableModels.length === 0 && <Text size="sm" c="dimmed">{t('probeNoModels')}</Text>}
        {models.isError && <InlineQueryError message={t('probeModelsUnavailable')} onRetry={() => void models.refetch()} />}
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
        <ProbeHistory rows={rows} />
      </Stack>
    </Modal>
  </>
}

const BULK_PROBE_CONCURRENCY = 3
type BulkProbeResult = { id: string; name: string; state: 'queued' | 'failed' | 'no-model' }
function BulkProbe() {
  const { t } = useTranslation()
  const { project } = useProject()
  const channels = useQuery({ queryKey: ['bulk-probe-channels', project.id], queryFn: () => api<Paged<Document>>(projectOperationPath(project.id, 'channels') + '?limit=500') })
  const models = useProbeModels()
  const [running, setRunning] = useState(false)
  const [results, setResults] = useState<BulkProbeResult[]>([])
  const displayedProject = useRef(project.id)
  displayedProject.current = project.id
  useEffect(() => { setRunning(false); setResults([]) }, [project.id])
  const run = async () => {
    const runProject = project.id
    const items = (channels.data?.data || []).filter((channel) => channel.enabled !== false).map((channel) => ({ channel, model: (models.data?.data || []).find((model) => model.provider_id === channel.id && model.enabled !== false && model.lifecycle !== 'archived') }))
    setResults(items.filter((item) => !item.model).map((item) => ({ id: item.channel.id, name: displayValue(item.channel.name || item.channel.id), state: 'no-model' as const })))
    const eligible = items.filter((item): item is { channel: Document; model: ProbeModel } => Boolean(item.model))
    setRunning(true)
    let cursor = 0
    const worker = async () => {
      while (cursor < eligible.length) {
        const item = eligible[cursor++]
        const name = displayValue(item.channel.name || item.channel.id)
        try {
          await api(projectOperationPath(runProject, 'probe'), { method: 'POST', body: JSON.stringify({ provider_id: item.channel.id, model_id: item.model.id }) })
          if (displayedProject.current === runProject) setResults((current) => [...current, { id: item.channel.id, name, state: 'queued' }])
        } catch {
          if (displayedProject.current === runProject) setResults((current) => [...current, { id: item.channel.id, name, state: 'failed' }])
        }
      }
    }
    await Promise.all(Array.from({ length: Math.min(BULK_PROBE_CONCURRENCY, eligible.length) }, worker))
    if (displayedProject.current === runProject) setRunning(false)
  }
  if (channels.isLoading || models.isLoading) return <Skeleton height={76} mt="md" radius="sm" />
  if (channels.isError || models.isError) return <Paper withBorder p="md" mt="md"><InlineQueryError message={t('bulkProbeUnavailable')} onRetry={() => { void channels.refetch(); void models.refetch() }} /></Paper>
  if (!channels.data?.data?.length) return null
  return <Paper withBorder p="md" mt="md">
    <Group justify="space-between" gap="md" wrap="wrap"><Stack gap={0}><Text fw={620}>{t('bulkProbe')}</Text><Text size="sm" c="dimmed">{t('bulkProbeHint', { count: BULK_PROBE_CONCURRENCY })}</Text></Stack><Button onClick={() => void run()} loading={running} disabled={models.isError || channels.isError}>{t('testAllChannels')}</Button></Group>
    {results.length > 0 && <Stack gap={4} mt="sm" aria-label={t('bulkProbeResults')}>{results.map((result) => <Text size="sm" key={result.id}>{`${result.name}: ${t(result.state === 'queued' ? 'bulkProbeQueued' : result.state === 'no-model' ? 'bulkProbeNoModel' : 'statusFailed')}`}</Text>)}</Stack>}
  </Paper>
}

function Diagnostics({ quota = false }: { quota?: boolean }) {
  const { t } = useTranslation()
  const { project } = useProject()
  const { options, query } = useChannelOptions()
  const models = useProbeModels()
  const [selected, setSelected] = useState('')
  const [selectedModel, setSelectedModel] = useState('')
  const current = selected || options[0]?.value || ''
  const availableModels = (models.data?.data || []).filter((model) => model.provider_id === current && model.enabled !== false && model.lifecycle !== 'archived')
  const currentModel = availableModels.some((model) => model.id === selectedModel) ? selectedModel : availableModels[0]?.id || ''
  const run = useMutation({ mutationFn: ({ provider_id, model_id }: { provider_id: string; model_id?: string }) => api(projectOperationPath(project.id, quota ? 'quota' : 'probe'), { method: 'POST', body: JSON.stringify({ provider_id, ...(model_id ? { model_id } : {}) }) }), onSuccess: () => toast.success(t('jobQueued')), onError: (error: Error) => toast.error(error.message) })
  return (
    <Paper withBorder p="md" mb="md">
      <form onSubmit={(event) => { event.preventDefault(); run.mutate({ provider_id: current, ...(quota ? {} : { model_id: currentModel }) }) }}>
        <Group gap="md" align="end" wrap="wrap">
          {/* The probe targets a channel, so a channel list that could not be read
              is reported here rather than as an empty picker. */}
          {query.isError
            ? <InlineQueryError message={t('optionsUnavailable')} onRetry={() => void query.refetch()} />
            : <SelectField label={t('channel')} value={current} onValueChange={setSelected} options={options} />}
          {!quota && (models.isError ? <InlineQueryError message={t('probeModelsUnavailable')} onRetry={() => void models.refetch()} /> : <SelectField label={t('model')} value={currentModel} onValueChange={setSelectedModel} options={availableModels.map((model) => ({ value: model.id, label: `${model.public_name || model.id}${model.upstream_name && model.upstream_name !== model.public_name ? ` → ${model.upstream_name}` : ''}` }))} />)}
          {!quota && models.isLoading && <Text size="sm" c="dimmed">{t('loading')}</Text>}
          {!quota && !models.isLoading && !models.isError && availableModels.length === 0 && <Text size="sm" c="dimmed">{t('probeNoModels')}</Text>}
          <Button type="submit" disabled={!current || query.isError || (!quota && (!currentModel || models.isError))}>{quota ? t('collectQuota') : t('runProbe')}</Button>
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
