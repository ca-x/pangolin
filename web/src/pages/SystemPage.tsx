import { ActionIcon, Alert, Badge, Button, Checkbox, Code, Group, Input, Modal, Pagination, Paper, Select, SimpleGrid, Stack, Switch, Table, TableScrollContainer, Tabs, Text, TextInput, Textarea, Title, Tooltip } from '@mantine/core'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { AlertTriangle, Copy, DatabaseBackup, Download, History, Play, RotateCcw, ShieldAlert, Webhook } from 'lucide-react'
import { useMemo, useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { api, type Bootstrap, type Document, type Paged } from '../api'
import { confirmAction, EmptyState, EnabledPill, InlineQueryError, SecretInput, SelectField, SkeletonRows } from '../components'
import { restoreOnboarding } from '../onboarding'
import { palettes } from '../palettes'
import { useErrorCodeLabel } from '../observability'
import { projectOperationPath, useProject } from '../project'
import { resolveSkin, skinLabel, skins } from '../skins'
import { useTheme, type ColorMode } from '../theme'
import { AutoRefreshControl, formatUpdatedAt, useAutoRefreshInterval } from './autoRefresh'
import { PageHeader, QueryError, ResourcePage, displayValue, formatDate, type FormField } from './shared'

/**
 * Project-scoped tabs read and write rows that belong to one project, so their
 * routes authorize with `project:manage`. Instance-scoped ones act on the whole
 * instance and authorize with the owner scope (`*`). The two sets are separate
 * lists because a principal can hold one and not the other, and the console must
 * not offer a tab whose calls would be refused.
 */
const PROJECT_TABS = ['orchestrationSettings', 'storage', 'backups', 'webhooks', 'jobs', 'retention'] as const

export default function SystemPage() {
  const { t } = useTranslation()
  const { permissions } = useProject()
  const can = (permission: string) => permissions.has('*') || permissions.has(permission)
  const projectScoped = can('project:manage')
  const instanceScoped = can('*')
  return (
    <Tabs keepMounted={false} defaultValue="appearance">
      <Tabs.List mb="lg">
        <Tabs.Tab value="appearance">{t('appearance')}</Tabs.Tab>
        {projectScoped && PROJECT_TABS.map((value) => <Tabs.Tab key={value} value={value}>{t(value)}</Tabs.Tab>)}
        {instanceScoped && <Tabs.Tab value="requestLogging">{t('requestLogging')}</Tabs.Tab>}
        <Tabs.Tab value="about">{t('about')}</Tabs.Tab>
      </Tabs.List>
      {!projectScoped && !instanceScoped && (
        <Alert variant="light" color="yellow" radius="lg" icon={<ShieldAlert />} mb="lg">{t('systemNoScope')}</Alert>
      )}
      <Tabs.Panel value="appearance"><Appearance /><BrandingSettings /></Tabs.Panel>
      {projectScoped && (
        <>
          <Tabs.Panel value="orchestrationSettings"><ProjectScope /><OrchestrationSettings /></Tabs.Panel>
          <Tabs.Panel value="storage">
            <ProjectScope />
            <ResourcePage resource="storage" title={t('storage')} description={t('storageDescription')} empty={t('storageEmpty')} createLabel={t('addStorage')} columns={[{ key: 'name', label: t('name') }, { key: 'kind', label: t('type') }, { key: 'config', label: t('configuration'), mono: true, render: (value) => storageSummary(value) }, { key: 'revision', label: t('revision') }, { key: 'enabled', label: t('status'), render: (value) => <EnabledPill enabled={value} /> }]} fields={STORAGE_FIELDS(t)} normalize={(values, editing) => storageBody(values, editing, t)} />
          </Tabs.Panel>
          <Tabs.Panel value="backups">
            <ProjectScope />
            {instanceScoped && <InstanceBackupPanel />}
            <BackupPanel />
          </Tabs.Panel>
          <Tabs.Panel value="webhooks">
            <ProjectScope />
            <ResourcePage resource="webhooks" title={t('webhooks')} description={t('webhooksDescription')} empty={t('webhookEmpty')} createLabel={t('addWebhook')} columns={[{ key: 'name', label: t('name') }, { key: 'url', label: 'URL', mono: true }, { key: 'subscriptions', label: t('events') }, { key: 'enabled', label: t('status'), render: (value) => <EnabledPill enabled={value} /> }]} fields={[{ key: 'name', label: t('name'), required: true }, { key: 'url', label: 'URL', required: true }, { key: 'events', sourceKey: 'subscriptions', label: t('events'), kind: 'json', defaultValue: ['channel.disabled', 'quota.exhausted', 'request.failed'] }, { key: 'headers', label: t('publicHeaders'), kind: 'json', defaultValue: {} }, { key: 'secret_headers', label: t('secretHeaders'), kind: 'json', omitWhenBlank: true }, { key: 'body', label: t('bodyTemplate'), kind: 'json', defaultValue: { body: '$event' } }, { key: 'enabled', label: t('enabled'), kind: 'checkbox' }]} />
            <WebhookDeliveries />
          </Tabs.Panel>
          <Tabs.Panel value="jobs"><ProjectScope /><JobsPanel /></Tabs.Panel>
          <Tabs.Panel value="retention">
            <ProjectScope />
            <ResourcePage resource="retention" title={t('retention')} description={t('retentionDescription')} empty={t('retentionEmpty')} createLabel={t('addPolicy')} columns={[{ key: 'resource_type', label: t('type') }, { key: 'retention_days', label: t('retentionDays') }, ]} fields={[{ key: 'resource_type', label: t('type'), kind: 'select', options: ['requests', 'payloads', 'probes', 'quota'].map((value) => ({ value, label: t(value) })) }, { key: 'retention_days', label: t('retentionDays'), kind: 'number', defaultValue: 30 }]} />
          </Tabs.Panel>
        </>
      )}
      {instanceScoped && <Tabs.Panel value="requestLogging"><LoggingPolicy /></Tabs.Panel>}
      <Tabs.Panel value="about"><About /></Tabs.Panel>
    </Tabs>
  )
}

/** Every project-scoped tab states which project it acts on. */
function ProjectScope() {
  const { t } = useTranslation()
  const { project } = useProject()
  return <Text size="sm" c="dimmed" mb="md">{t('projectScope', { project: project.name })}</Text>
}

/** Identity of the console bundle itself, injected by `vite.config.ts` at build time. */
export type WebBuild = { version: string; commit: string; builtAt: string }

/**
 * The console's own build identity. Returns `null` when the bundle was built
 * without the injection, so the About panel degrades to `—` instead of throwing.
 */
export function consoleBuild(): WebBuild | null {
  return typeof __WEB_BUILD__ === 'undefined' ? null : __WEB_BUILD__
}

/**
 * Two builds are the same revision when their short commits match. A trailing
 * `-dirty` only records uncommitted changes at build time, and a commit neither
 * side could measure must not raise a false alarm.
 */
export function commitsDiffer(backend?: string | null, web?: string | null): boolean {
  const known = (value?: string | null) => {
    const trimmed = value?.trim().replace(/-dirty$/, '') ?? ''
    return trimmed === 'unknown' ? '' : trimmed
  }
  const left = known(backend)
  const right = known(web)
  return left !== '' && right !== '' && left !== right
}

/**
 * Build times arrive as RFC 3339 UTC. Render them in the reader's local format;
 * a value that is not a timestamp is shown verbatim rather than invented.
 */
export function formatBuildTime(value: string | null | undefined, language: string): string {
  if (!value) return '—'
  const moment = new Date(value)
  if (Number.isNaN(moment.getTime())) return value
  return new Intl.DateTimeFormat(language, { dateStyle: 'medium', timeStyle: 'short' }).format(moment)
}

function BuildFacts({ title, facts }: { title: string; facts: Array<{ label: string; value: string }> }) {
  return (
    <Paper withBorder p="lg">
      <Title order={2} mb="md">{title}</Title>
      <Stack component="dl" gap="sm" m={0}>
        {facts.map((fact) => (
          <Group key={fact.label} component="div" justify="space-between" align="flex-start" gap="md" wrap="nowrap">
            <Text component="dt" size="sm" c="dimmed" fw={600} style={{ flex: '0 0 auto' }}>{fact.label}</Text>
            <Text component="dd" size="md" ff="monospace" style={{ margin: 0, minWidth: 0, textAlign: 'right', overflowWrap: 'anywhere' }}>{fact.value}</Text>
          </Group>
        ))}
      </Stack>
    </Paper>
  )
}

/**
 * The release binary embeds `web/dist` at compile time, so a binary and a console
 * built from different revisions are indistinguishable from the outside. This
 * panel is where that pair is compared.
 */
function About() {
  const { t, i18n } = useTranslation()
  const query = useQuery({ queryKey: ['bootstrap'], queryFn: () => api<Bootstrap>('/api/v1/bootstrap'), retry: false })
  if (query.isError) return <QueryError retry={() => void query.refetch()} />
  if (query.isLoading || !query.data) return <SkeletonRows />
  const backend = query.data.build
  const bundle = consoleBuild()
  const mismatch = commitsDiffer(backend?.commit, bundle?.commit)
  const diagnostics = [
    t('productName'),
    `backend.version=${displayValue(backend?.version)}`,
    `backend.commit=${displayValue(backend?.commit)}`,
    `backend.built_at=${displayValue(backend?.built_at)}`,
    `backend.target=${displayValue(backend?.target)}`,
    `backend.profile=${displayValue(backend?.profile)}`,
    `console.version=${displayValue(bundle?.version)}`,
    `console.commit=${displayValue(bundle?.commit)}`,
    `console.builtAt=${displayValue(bundle?.builtAt)}`,
  ].join('\n')
  const copyDiagnostics = () => {
    const written = navigator.clipboard?.writeText(diagnostics)
    if (!written) { toast.error(t('copyFailed')); return }
    void written.then(() => toast.success(t('copied')), () => toast.error(t('copyFailed')))
  }
  return (
    <>
      <PageHeader title={t('about')} description={t('aboutDescription')} />
      {mismatch && (
        <Alert variant="light" color="yellow" radius="lg" title={t('buildMismatchTitle')} icon={<AlertTriangle />} mb="lg">
          <Text size="md">{t('buildMismatchHint', { backend: displayValue(backend?.commit), console: displayValue(bundle?.commit) })}</Text>
        </Alert>
      )}
      <Paper withBorder p="lg" mb="md">
        <Title order={2}>{t('productName')}</Title>
        <Text size="md" c="dimmed" mt={4}>{t('productSubtitle')}</Text>
      </Paper>
      <SimpleGrid cols={{ base: 1, md: 2 }} spacing="md">
        <BuildFacts title={t('backendBuild')} facts={[
          { label: t('version'), value: displayValue(backend?.version) },
          { label: t('commit'), value: displayValue(backend?.commit) },
          { label: t('buildTime'), value: formatBuildTime(backend?.built_at, i18n.language) },
          { label: t('buildTarget'), value: displayValue(backend?.target) },
          { label: t('buildProfile'), value: displayValue(backend?.profile) },
        ]} />
        <BuildFacts title={t('consoleBuild')} facts={[
          { label: t('version'), value: displayValue(bundle?.version) },
          { label: t('commit'), value: displayValue(bundle?.commit) },
          { label: t('buildTime'), value: formatBuildTime(bundle?.builtAt, i18n.language) },
        ]} />
      </SimpleGrid>
      <Group mt="lg">
        <Button variant="default" leftSection={<Copy size={16} />} onClick={copyDiagnostics}>{t('copyDiagnostics')}</Button>
      </Group>
    </>
  )
}

function Appearance() {
  const { t, i18n } = useTranslation()
  const { mode, skin, palette, setMode, setSkin, setPalette } = useTheme()
  const current = resolveSkin(skin)
  return (
    <>
      <PageHeader title={t('appearance')} description={t('appearanceDescription')} />
      <Paper withBorder p="lg">
        <Stack gap="md">
          <div>
            <Title order={2}>{t('interface')}</Title>
            <Text size="sm" c="dimmed" mt={4}>{t('appearanceHint')}</Text>
          </div>
          <SimpleGrid cols={{ base: 1, sm: 2, lg: 4 }} spacing="md">
            <Select label={t('colorMode')} value={mode} onChange={(value) => setMode((value as ColorMode) ?? 'system')} data={(['system', 'light', 'dark'] as const).map((value) => ({ value, label: t(value) }))} allowDeselect={false} />
            <Select label={t('skin')} value={skin} onChange={(value) => value && setSkin(value)} data={skins.map((entry) => ({ value: entry.id, label: skinLabel(entry.id, i18n.language) }))} allowDeselect={false} searchable />
            <Select label={t('palette')} value={palette} onChange={(value) => value && setPalette(value)} data={palettes.map((entry) => ({ value: entry.id, label: i18n.language.startsWith('zh') ? entry.labelZh : entry.labelEn }))} allowDeselect={false} searchable />
            <Select label={t('language')} value={i18n.language} onChange={(value) => value && void i18n.changeLanguage(value)} data={[{ value: 'zh-CN', label: '简体中文' }, { value: 'en', label: 'English' }]} allowDeselect={false} />
          </SimpleGrid>
          <Text size="sm" c="dimmed">{t('skinHint', { basis: current.basis === 'house' ? 'Pangolin' : current.basis, material: current.material, mode: current.preferredMode })}</Text>
        </Stack>
      </Paper>
    </>
  )
}

type Branding = { instance_name: string; branding_name: string; favicon_url: string; onboarding_complete: boolean }
function BrandingSettings() {
  const { t } = useTranslation()
  const client = useQueryClient()
  const query = useQuery({ queryKey: ['system-settings'], queryFn: () => api<Branding>('/api/admin/v1/settings/system') })
  const save = useMutation({ mutationFn: (body: Branding) => api('/api/admin/v1/settings/system', { method: 'PUT', body: JSON.stringify(body) }), onSuccess: () => { toast.success(t('saved')); void client.invalidateQueries({ queryKey: ['system-settings'] }); void client.invalidateQueries({ queryKey: ['bootstrap'] }) }, onError: (error: Error) => toast.error(error.message) })
  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const data = new FormData(event.currentTarget);
    save.mutate({ instance_name: String(data.get('instance_name')), branding_name: String(data.get('branding_name')), favicon_url: String(data.get('favicon_url')), onboarding_complete: data.get('onboarding_complete') === 'on' })
  }
  const branding = query.data
  return (
    <Paper withBorder p="lg" mt="lg">
      {query.isError
        ? <QueryError retry={() => void query.refetch()} />
        : (
          <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="xl">
            <Stack gap="xs">
              <Title order={2}>{t('branding')}</Title>
              <Text size="sm" c="dimmed">{t('brandingHint')}</Text>
            </Stack>
            {branding ? (
              <form onSubmit={submit}>
                <Stack gap="md">
                  <TextInput name="instance_name" label={t('instanceName')} defaultValue={branding.instance_name} required />
                  <TextInput name="branding_name" label={t('brandingName')} defaultValue={branding.branding_name} required />
                  <TextInput name="favicon_url" label={t('faviconUrl')} defaultValue={branding.favicon_url} required />
                  <Checkbox name="onboarding_complete" label={t('onboardingComplete')} defaultChecked={branding.onboarding_complete} />
                  <Button type="submit">{t('save')}</Button>
                </Stack>
              </form>
            ) : <SkeletonRows count={2} />}
          </SimpleGrid>
        )}
      {/* Undoing a dismissal is a browser-local choice, so it stays reachable
          whether or not the branding record could be read. */}
      <Group justify="space-between" align="center" gap="md" wrap="wrap" mt="lg" pt="md" style={{ borderTop: '1px solid var(--mantine-color-default-border)' }}>
        <Stack gap={0}>
          <Text fw={600}>{t('onboardingRestoreTitle')}</Text>
          <Text size="sm" c="dimmed">{t('onboardingRestoreHint')}</Text>
        </Stack>
        <Button variant="default" onClick={() => { restoreOnboarding(); toast.success(t('onboardingRestored')) }}>{t('onboardingRestore')}</Button>
      </Group>
    </Paper>
  )
}

type OrchestrationDocument = { version: number; affinity_rules: unknown[]; session_compaction: Record<string, unknown>; routing?: unknown }
function OrchestrationSettings() {
  const { t } = useTranslation()
  const { project } = useProject()
  const client = useQueryClient()
  const path = `/api/admin/v1/projects/${project.id}/settings/orchestration`
  const query = useQuery({ queryKey: ['orchestration-settings', project.id], queryFn: () => api<OrchestrationDocument>(path) })
  const save = useMutation({ mutationFn: (body: OrchestrationDocument) => api(path, { method: 'PUT', body: JSON.stringify(body) }), onSuccess: () => { toast.success(t('saved')); void client.invalidateQueries({ queryKey: ['orchestration-settings', project.id] }) }, onError: (error: Error) => toast.error(error.message) })
  if (query.isLoading || !query.data) return <SkeletonRows />
  if (query.isError) return <QueryError retry={() => void query.refetch()} />
  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const data = new FormData(event.currentTarget);
    try {
      save.mutate({ version: 1, affinity_rules: JSON.parse(String(data.get('affinity_rules'))), session_compaction: JSON.parse(String(data.get('session_compaction'))), routing: JSON.parse(String(data.get('routing'))) })
    } catch { toast.error(t('invalidJson')) }
  }
  return (
    <>
      <PageHeader title={t('orchestrationSettings')} description={t('orchestrationSettingsHint')} />
      <Paper withBorder p="lg">
        <form onSubmit={submit}>
          <Stack gap="md">
            <Textarea name="affinity_rules" label={t('affinityRules')} description={t('affinityRulesHint')} rows={12} defaultValue={JSON.stringify(query.data.affinity_rules, null, 2)} required />
            <Textarea name="session_compaction" label={t('sessionCompaction')} description={t('sessionCompactionHint')} rows={10} defaultValue={JSON.stringify(query.data.session_compaction, null, 2)} required />
            {/* The policy was in the document all along; the form neither showed
                nor sent it, so the console could not edit a project's routing. */}
            <Textarea name="routing" label={t('routingPolicy')} description={t('routingPolicyHint')} rows={10} defaultValue={JSON.stringify(query.data.routing ?? { version: 1 }, null, 2)} required />
            <Button type="submit">{t('save')}</Button>
          </Stack>
        </form>
      </Paper>
    </>
  )
}

type Policy = { enabled: boolean; default_level: string; key_override_enabled: boolean; key_disable_allowed: boolean }
function LoggingPolicy() {
  const { t } = useTranslation()
  const client = useQueryClient()
  const query = useQuery({ queryKey: ['logging-policy'], queryFn: () => api<Policy>('/api/admin/v1/settings/request-logging') })
  const update = useMutation({ mutationFn: (policy: Policy) => api('/api/admin/v1/settings/request-logging', { method: 'PUT', body: JSON.stringify(policy) }), onSuccess: () => { toast.success(t('saved')); void client.invalidateQueries({ queryKey: ['logging-policy'] }) }, onError: (error: Error) => toast.error(error.message) })
  if (query.isError) return <QueryError retry={() => void query.refetch()} />
  if (query.isLoading || !query.data) return <SkeletonRows />
  const policy = query.data
  return (
    <>
      <PageHeader title={t('requestLogging')} description={t('requestLoggingDescription')} />
      <Paper withBorder p="lg">
        <Stack gap="md">
          <Switch checked={policy.enabled} onChange={(event) => update.mutate({ ...policy, enabled: event.currentTarget.checked })} label={t('enableRequestLogging')} description={t('enableRequestLoggingHint')} />
          <SelectField label={t('defaultLevel')} value={policy.default_level} onValueChange={(value) => update.mutate({ ...policy, default_level: value })} options={['off', 'metadata', 'redacted_body', 'full_body'].map((value) => ({ value, label: t(value) }))} />
          <Switch checked={policy.key_override_enabled} onChange={(event) => update.mutate({ ...policy, key_override_enabled: event.currentTarget.checked })} label={t('allowKeyOverrides')} />
          <Switch checked={policy.key_disable_allowed} onChange={(event) => update.mutate({ ...policy, key_disable_allowed: event.currentTarget.checked })} label={t('allowKeyDisable')} />
        </Stack>
      </Paper>
    </>
  )
}

/**
 * Selectable backup tables. `src/operations/backup.rs::TABLES` validates every
 * name against this allowlist and rejects anything else, so the console offers
 * the same names instead of asking for them as free text; a name the server
 * dropped is refused with its own message rather than being silently ignored.
 * Grouping follows `docs/operations.md` (configuration versus request history).
 */
const BACKUP_GROUPS: Array<{ label: string; tables: string[] }> = [
  { label: 'backupGroupChannels', tables: ['providers', 'channel_credentials', 'channel_settings', 'models', 'model_associations', 'model_prices', 'model_price_components'] },
  { label: 'backupGroupAccess', tables: ['api_key_profiles', 'api_key_profile_model_mappings', 'api_key_profile_allowed_models', 'api_keys', 'prompts', 'prompt_protection_rules', 'service_groups', 'service_group_keys', 'service_group_channels'] },
  { label: 'backupGroupOperations', tables: ['webhooks', 'data_retention_policies', 'provider_quota_snapshots', 'channel_probes'] },
  { label: 'backupGroupHistory', tables: ['threads', 'traces', 'requests', 'request_contents', 'request_facts', 'execution_facts', 'request_executions', 'usage_logs', 'usage_cost_items', 'provider_response_settlements', 'response_sessions', 'session_summaries'] },
]
const ALL_BACKUP_TABLES = BACKUP_GROUPS.flatMap((group) => group.tables)
/** Configuration and operations: the set an operator wants by default, matching `include_history:false`. */
const CONFIGURATION_BACKUP_TABLES = BACKUP_GROUPS.filter((group) => group.label !== 'backupGroupHistory').flatMap((group) => group.tables)

type BackupArtifact = { version: number; id: string; project_id: string; created_at: number; digest: string; resources: string[]; envelope: string }
/** A row of the artifact history: metadata only, no encrypted payload. */
type BackupArtifactSummary = { id: string; digest: string; created_at: number; manifest: { version: number; resources: string[] } }
type JobRow = { id: string; kind: string; status: string; attempts: number; due_at: number; created_at: number; error_code: string | null }
type BackupTargetResult = { id: string; job_id: string; storage_id: string; status: string; error_code: string | null; byte_size: number | null; object_key: string | null }

/**
 * A restore input is a project-bound artifact, and the server refuses anything
 * that is not one. Reading the shape here turns a mis-picked file into a named
 * error instead of a request that fails after the operator confirmed a
 * destructive action.
 */
export function readBackupArtifact(value: unknown): BackupArtifact | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null
  const candidate = value as Partial<BackupArtifact>
  if (candidate.version !== 1) return null
  if (typeof candidate.id !== 'string' || typeof candidate.project_id !== 'string' || typeof candidate.envelope !== 'string') return null
  if (!Array.isArray(candidate.resources) || candidate.resources.some((name) => typeof name !== 'string')) return null
  return { version: 1, id: candidate.id, project_id: candidate.project_id, created_at: Number(candidate.created_at) || 0, digest: String(candidate.digest ?? ''), resources: candidate.resources as string[], envelope: candidate.envelope }
}

/** Saves a JSON document under the browser's download directory. */
export function downloadJson(name: string, value: unknown) {
  const body = JSON.stringify(value, null, 2)
  // `createObjectURL` is the supported path; a data URL keeps the action working
  // in environments that do not implement it (jsdom in the test suite).
  const url = typeof URL.createObjectURL === 'function'
    ? URL.createObjectURL(new Blob([body], { type: 'application/json' }))
    : `data:application/json;charset=utf-8,${encodeURIComponent(body)}`
  const anchor = document.createElement('a')
  anchor.href = url
  anchor.download = name
  anchor.click()
  if (typeof URL.revokeObjectURL === 'function' && url.startsWith('blob:')) URL.revokeObjectURL(url)
}

const backupFileName = (slug: string, id: string) => `pangolin-backup-${slug || 'project'}-${id.slice(0, 8)}.json`

/** The `config` of a storage row, in the terms the form uses. */
function storageSummary(config: unknown): string {
  const value = (config && typeof config === 'object' ? config : {}) as Record<string, unknown>
  if (value.kind === 'local') return displayValue(value.directory)
  if (value.kind === 's3') return `${displayValue(value.bucket)} @ ${displayValue(value.endpoint)}${value.prefix ? ` · ${value.prefix}` : ''}`
  return displayValue(config)
}

const configOf = (row: Document | null) => (row?.config && typeof row.config === 'object' ? row.config : {}) as Record<string, unknown>
const payloadOf = (row: Document | null) => (row?.payload && typeof row.payload === 'object' ? row.payload : {}) as Record<string, unknown>

/** `src/operations/storage.rs` accepts exactly these two config shapes and nothing else. */
const STORAGE_DIRECTORY = /^[A-Za-z0-9_-]{1,128}$/
const safeStoragePrefix = (value: string) => value.length <= 256 && value.split('/').every((segment) => segment !== '.' && segment !== '..' && /^[A-Za-z0-9_-]*$/.test(segment))

/**
 * Every key the storage resource accepts: `name`, a `config` in one of its two
 * shapes, the optionally encrypted `secret`, `enabled` and the row's `revision`.
 * The credential fields are the visible path to the S3 secret; the JSON box is
 * the escape hatch for anything they do not cover.
 */
function STORAGE_FIELDS(t: (key: string, options?: Record<string, unknown>) => string): FormField[] {
  return [
    { key: 'name', label: t('name'), required: true },
    { key: 'kind', label: t('type'), kind: 'select', options: [{ value: 'local', label: t('storageLocal') }, { value: 's3', label: t('storageS3') }] },
    { key: 'directory', label: t('storageDirectory'), hint: t('storageDirectoryHint'), omitWhenBlank: true, fromRow: (row) => configOf(row).directory },
    { key: 'endpoint', label: t('storageEndpoint'), hint: t('storageEndpointHint'), omitWhenBlank: true, fromRow: (row) => configOf(row).endpoint },
    { key: 'bucket', label: t('storageBucket'), omitWhenBlank: true, fromRow: (row) => configOf(row).bucket },
    { key: 'region', label: t('storageRegion'), omitWhenBlank: true, fromRow: (row) => configOf(row).region },
    { key: 'prefix', label: t('storagePrefix'), hint: t('storagePrefixHint'), omitWhenBlank: true, fromRow: (row) => configOf(row).prefix },
    { key: 'access_key_id', label: t('storageAccessKeyId'), kind: 'secret' },
    { key: 'secret_access_key', label: t('storageSecretAccessKey'), kind: 'secret' },
    { key: 'session_token', label: t('storageSessionToken'), kind: 'secret' },
    { key: 'secret', label: t('storageSecret'), hint: t('storageSecretJsonHint'), kind: 'json', omitWhenBlank: true },
    { key: 'enabled', label: t('enabled'), kind: 'checkbox' },
    { key: 'revision', label: t('revision'), kind: 'number', defaultValue: 0 },
  ]
}

/**
 * The S3 credential envelope. A blank pair keeps the envelope the row already
 * has (`secret_envelope=COALESCE(?,secret_envelope)` on the API side), and a new
 * S3 target cannot be created without one: delivery would fail when the store is
 * opened, which the operator would only discover from a failed run.
 */
function storageSecret(values: Record<string, unknown>, kind: string, editing: boolean, t: (key: string, options?: Record<string, unknown>) => string): Record<string, unknown> | null {
  const accessKey = String(values.access_key_id ?? '').trim()
  const secretKey = String(values.secret_access_key ?? '').trim()
  const sessionToken = String(values.session_token ?? '').trim()
  const override = values.secret && typeof values.secret === 'object' && !Array.isArray(values.secret) ? values.secret as Record<string, unknown> : null
  if (accessKey || secretKey) {
    if (!accessKey || !secretKey) throw new Error(t('storageCredentialsIncomplete'))
    return { access_key_id: accessKey, secret_access_key: secretKey, ...(sessionToken ? { session_token: sessionToken } : {}) }
  }
  if (override) {
    if (kind === 's3' && (typeof override.access_key_id !== 'string' || typeof override.secret_access_key !== 'string')) throw new Error(t('storageCredentialsIncomplete'))
    return override
  }
  if (kind === 's3' && !editing) throw new Error(t('storageCredentialsRequired'))
  return null
}

/**
 * Builds the storage body from the form's fields: the row's id when editing (the
 * collection route upserts on it), `name`, a `config` in one of its two shapes,
 * the optionally encrypted `secret`, `enabled` and the row's `revision`. The API
 * validates the same rules and answers a violation with one generic message, so
 * the form reports which value is wrong instead of sending a request that will be
 * refused.
 */
function storageBody(values: Record<string, unknown>, editing: Document | null, t: (key: string, options?: Record<string, unknown>) => string): Record<string, unknown> {
  const previous = configOf(editing)
  const kind = String(values.kind ?? previous.kind ?? 'local')
  const field = (key: string, fallback: unknown) => String(values[key] ?? fallback ?? '').trim()
  let config: Record<string, unknown>
  if (kind === 's3') {
    const endpoint = field('endpoint', previous.endpoint)
    let url: URL | null = null
    try { url = new URL(endpoint) } catch { url = null }
    if (!url || url.protocol !== 'https:' || url.hostname === '' || url.username !== '' || url.password !== '' || url.search !== '' || url.hash !== '') throw new Error(t('storageEndpointInvalid'))
    const bucket = field('bucket', previous.bucket)
    const region = field('region', previous.region)
    if (!bucket || !region) throw new Error(t('storageBucketRegionRequired'))
    const prefix = field('prefix', previous.prefix)
    if (!safeStoragePrefix(prefix)) throw new Error(t('storagePrefixInvalid'))
    config = { kind: 's3', endpoint, bucket, region, prefix }
  } else {
    const directory = field('directory', previous.directory)
    if (!STORAGE_DIRECTORY.test(directory)) throw new Error(t('storageDirectoryInvalid'))
    config = { kind: 'local', directory }
  }
  const secret = storageSecret(values, kind, editing !== null, t)
  return { ...(editing ? { id: editing.id } : {}), name: values.name, config, enabled: values.enabled, revision: values.revision, ...(secret ? { secret } : {}) }
}

/**
 * The two payload shapes `src/operations/runtime.rs` reads — `{targets,
 * resources}` for an automatic backup and `{storage_id, keep}` for retention —
 * plus the JSON box, which stays the escape hatch for the other kinds. The
 * retention fields are what an operator edits; the API refuses a count outside
 * 1–1000 at run time, so the form refuses it here. The row's id travels with the
 * body, because the collection route upserts on it.
 */
function scheduleBody(values: Record<string, unknown>, editing: Document | null, t: (key: string, options?: Record<string, unknown>) => string): Record<string, unknown> {
  // The server accepts 30 s to 365 days and answers anything else with a bare
  // "invalid schedule interval", which the console used to pass straight through.
  const interval = Number(values.interval_secs)
  if (values.interval_secs !== undefined && values.interval_secs !== '' && !(interval >= 30 && interval <= 31_536_000)) {
    throw new Error(t('scheduleIntervalRange'))
  }
  const previous = payloadOf(editing)
  const { storage_id, keep, ...rest } = values
  const row = editing ? { id: editing.id } : {}
  if (rest.kind !== 'backup_retention') {
    // Every other kind is defined by its payload, so the JSON box is the only
    // place it can come from and it has to be an object.
    if (!rest.payload || typeof rest.payload !== 'object' || Array.isArray(rest.payload)) throw new Error(t('schedulePayloadRequired'))
    return { ...row, ...rest }
  }
  // A retention payload is exactly these two values, so the JSON box is not
  // required and cannot contradict them.
  const storage = String(storage_id ?? previous.storage_id ?? '')
  if (!storage) throw new Error(t('backupRetentionNeedsStorage'))
  const count = Number(keep ?? previous.keep ?? 7)
  if (!Number.isInteger(count) || count < 1 || count > 1000) throw new Error(t('backupRetentionCountRange'))
  return { ...row, ...rest, payload: { storage_id: storage, keep: count } }
}

function BackupResourcePicker({ selected, onChange }: { selected: string[]; onChange: (next: string[]) => void }) {
  const { t } = useTranslation()
  const toggle = (table: string, checked: boolean) => onChange(checked ? [...selected, table] : selected.filter((name) => name !== table))
  return (
    <Stack gap="sm">
      <Group justify="space-between" align="flex-end" gap="sm" wrap="wrap">
        <div>
          <Text fw={600}>{t('backupResources')}</Text>
          <Text size="sm" c="dimmed">{t('backupResourcesHint')}</Text>
        </div>
        <Group gap="xs">
          <Button variant="default" size="compact-sm" onClick={() => onChange(CONFIGURATION_BACKUP_TABLES)}>{t('backupResourcesConfiguration')}</Button>
          <Button variant="default" size="compact-sm" onClick={() => onChange(ALL_BACKUP_TABLES)}>{t('backupResourcesAll')}</Button>
          <Button variant="default" size="compact-sm" onClick={() => onChange([])}>{t('backupResourcesNone')}</Button>
        </Group>
      </Group>
      <Text size="sm" c="dimmed">{t('backupResourcesSelected', { count: selected.length, total: ALL_BACKUP_TABLES.length })}</Text>
      <SimpleGrid cols={{ base: 1, sm: 2, lg: 4 }} spacing="md">
        {BACKUP_GROUPS.map((group) => (
          <Stack key={group.label} gap={4}>
            <Text size="sm" fw={600}>{t(group.label)}</Text>
            {group.tables.map((table) => (
              <Checkbox
                key={table}
                size="sm"
                label={<span className="mono-cell">{table}</span>}
                checked={selected.includes(table)}
                onChange={(event) => toggle(table, event.currentTarget.checked)}
              />
            ))}
          </Stack>
        ))}
      </SimpleGrid>
    </Stack>
  )
}

function BackupPanel() {
  const errorLabel = useErrorCodeLabel()
  const { t } = useTranslation()
  const { project } = useProject()
  const client = useQueryClient()
  const base = `/api/admin/v1/projects/${project.id}/backup`
  const [resources, setResources] = useState<string[]>(CONFIGURATION_BACKUP_TABLES)
  const [targets, setTargets] = useState<string[]>([])
  const [artifact, setArtifact] = useState<BackupArtifact | null>(null)
  const [file, setFile] = useState<File | null>(null)
  const [fileError, setFileError] = useState<string | null>(null)
  const [strategy, setStrategy] = useState('fail')
  // The project's artifact history. Downloads fetch the single artifact, which is
  // the only route that carries the encrypted payload.
  const artifacts = useQuery({
    queryKey: ['backup-artifacts', project.id],
    queryFn: () => api<{ artifacts: BackupArtifactSummary[] }>(backupPath(project.id, 'artifacts')),
  })
  const invalidateRuns = () => void client.invalidateQueries({ queryKey: ['backup-runs', project.id] })
  // Backup routes are not operation resources: they live at
  // /api/admin/v1/projects/{id}/backup/..., which is where the artifact list is.
  const backupPath = (projectId: string, resource: string) => `/api/admin/v1/projects/${encodeURIComponent(projectId)}/backup/${resource}`
  const fetchArtifact = async (id: string) => {
    try {
      // `get_artifact` requires the CSRF header even though this is a GET, and
      // `api()` only attaches it to state-changing methods, so the download was
      // refused with `permission denied`. The console sends the header rather than
      // the route dropping the check.
      const artifact = await api<BackupArtifact>(backupPath(project.id, `artifacts/${encodeURIComponent(id)}`), { headers: { 'X-Pangolin-CSRF': '1' } })
      downloadJson(backupFileName(project.slug, artifact.id), artifact)
    } catch (error) {
      toast.error((error as Error).message)
    }
  }

  const storage = useQuery({ queryKey: ['backup-targets', project.id], queryFn: () => api<Paged<Document>>(`${projectOperationPath(project.id, 'storage')}?limit=200`) })
  const targetOptions = (storage.data?.data || []).filter((row) => row.enabled !== false).map((row) => ({ id: String(row.id), name: displayValue(row.name), kind: displayValue(row.kind) }))
  // A retention schedule can only prune a target the API will open, which is an
  // enabled one, so the picker offers the same set a delivery can use.
  const storageOptions = (storage.data?.data || []).filter((row) => row.enabled !== false).map((row) => ({ value: String(row.id), label: displayValue(row.name) }))
  const runs = useQuery({ queryKey: ['backup-runs', project.id], queryFn: () => api<Paged<JobRow>>(`${projectOperationPath(project.id, 'jobs')}?limit=500`) })
  const results = useQuery({ queryKey: ['backup-run-results', project.id], queryFn: () => api<Paged<BackupTargetResult>>(`${projectOperationPath(project.id, 'backup-target-results')}?limit=500`) })

  const exportNow = useMutation({
    mutationFn: () => api<BackupArtifact>(`${base}/export`, { method: 'POST', body: JSON.stringify({ resources }) }),
    onSuccess: (created) => {
      const name = backupFileName(project.slug, created.id)
      downloadJson(name, created)
      void client.invalidateQueries({ queryKey: ['backup-artifacts', project.id] })
      toast.success(t('backupExported'), { description: t('backupExportedHint', { name, count: created.resources.length }) })
    },
    onError: (error: Error) => toast.error(error.message),
  })
  const restore = useMutation({
    mutationFn: (input: { artifact: BackupArtifact; strategy: string }) => api<{ restored: number }>(`${base}/restore`, { method: 'POST', body: JSON.stringify(input) }),
    onSuccess: (result) => {
      // A repeated import of the same artifact and strategy is a no-op the API reports as 0.
      if (result.restored > 0) toast.success(t('backupRestored', { count: result.restored }))
      else toast.success(t('backupRestoreNoop'))
      void client.invalidateQueries({ queryKey: ['resource'] })
    },
    onError: (error: Error) => toast.error(error.message),
  })
  const deliver = useMutation({
    mutationFn: () => api<{ id: string }>(`${base}/run`, { method: 'POST', body: JSON.stringify({ targets, resources }) }),
    // Queuing a delivery leaves the restore input untouched: the run is a new
    // durable job, not a replacement for the artifact the operator selected.
    onSuccess: (job) => { toast.success(t('backupQueued', { id: job.id })); invalidateRuns() },
    onError: (error: Error) => toast.error(error.message),
  })
  const retry = useMutation({
    mutationFn: (job: string) => api<{ id: string }>(`${base}/${job}/retry`, { method: 'POST', body: '{}' }),
    onSuccess: (job) => { toast.success(t('backupRetryQueued', { id: job.id })); invalidateRuns() },
    onError: (error: Error) => toast.error(error.message),
  })

  const pickFile = async (picked: File | null) => {
    setFile(picked)
    setArtifact(null)
    setFileError(null)
    if (!picked) return
    try {
      const parsed = readBackupArtifact(JSON.parse(await picked.text()))
      if (!parsed) { setFileError(t('restoreFileInvalid')); return }
      setArtifact(parsed)
    } catch {
      setFileError(t('restoreFileInvalid'))
    }
  }
  const restoreDisabledReason = !artifact
    ? t('restoreNeedsFile')
    : artifact.project_id !== project.id
      ? t('restoreForeignArtifact', { project: artifact.project_id })
      : null
  const confirmRestore = () => {
    if (!artifact || restoreDisabledReason) return
    confirmAction({
      title: t('restoreBackupTitle', { project: project.name }),
      // The body names the value the API receives, which is what the operator is choosing.
      body: t('restoreBackupBody', { strategy }),
      confirmLabel: t('restore'),
      onConfirm: () => restore.mutateAsync({ artifact, strategy }),
    })
  }

  const backupRuns = useMemo(() => (runs.data?.data || []).filter((job) => job.kind === 'backup').sort((a, b) => b.created_at - a.created_at).slice(0, 25), [runs.data])
  const resultsByJob = useMemo(() => {
    const grouped = new Map<string, BackupTargetResult[]>()
    for (const row of results.data?.data || []) {
      const current = grouped.get(row.job_id) || []
      current.push(row)
      grouped.set(row.job_id, current)
    }
    return grouped
  }, [results.data])
  const targetName = (id: string) => targetOptions.find((option) => option.id === id)?.name || id

  return (
    <>
      <PageHeader title={t('backups')} description={t('backupsDescription')} />
      <Stack gap="lg">
        <Paper withBorder p="lg">
          <Stack gap="md">
            <Title order={2}>{t('exportBackup')}</Title>
            <BackupResourcePicker selected={resources} onChange={setResources} />
            <Group>
              <Tooltip label={t('backupNeedsResources')} disabled={resources.length > 0}>
                <span>
                  <Button leftSection={<Download size={17} />} loading={exportNow.isPending} disabled={resources.length === 0} onClick={() => exportNow.mutate()}>{t('export')}</Button>
                </span>
              </Tooltip>
            </Group>
            {artifacts.isError && <InlineQueryError message={t('backupArtifactsUnavailable')} onRetry={() => void artifacts.refetch()} />}
            {(artifacts.data?.artifacts?.length ?? 0) > 0 && (
              <Stack gap="xs">
                <Text size="sm" fw={600}>{t('sessionArtifacts')}</Text>
                <Text size="sm" c="dimmed">{t('sessionArtifactsHint')}</Text>
                {(artifacts.data?.artifacts ?? []).map((listed) => ({
                  // The list route carries a manifest; the panel renders the
                  // export shape, which keeps the resource names at the top level.
                  ...listed,
                  resources: listed.manifest?.resources ?? [],
                })).map((item) => (
                  <Group key={item.id} justify="space-between" gap="sm" wrap="nowrap">
                    <Text size="sm" className="mono-cell" style={{ minWidth: 0, overflowWrap: 'anywhere' }}>{item.id}</Text>
                    <Group gap="xs" wrap="nowrap">
                      <Text size="sm" c="dimmed">{t('backupArtifactSummary', { count: item.resources.length, created: formatDate(item.created_at) })}</Text>
                      <ActionIcon variant="subtle" color="gray" aria-label={`${t('download')} ${item.id}`} onClick={() => void fetchArtifact(item.id)}><Download size={16} /></ActionIcon>
                    </Group>
                  </Group>
                ))}
              </Stack>
            )}
          </Stack>
        </Paper>

        <Paper withBorder p="lg">
          <Stack gap="md">
            <Title order={2}>{t('deliverBackup')}</Title>
            <div>
              <Text fw={600}>{t('backupTargets')}</Text>
              <Text size="sm" c="dimmed">{t('backupTargetsHint')}</Text>
            </div>
            {storage.isError ? <QueryError retry={() => void storage.refetch()} /> : storage.isLoading ? <SkeletonRows count={1} /> : targetOptions.length === 0 ? (
              <Text size="sm" c="dimmed">{t('backupTargetEmpty')}</Text>
            ) : (
              <Stack gap={4}>
                {targetOptions.map((option) => (
                  <Checkbox
                    key={option.id}
                    label={`${option.name} · ${option.kind}`}
                    checked={targets.includes(option.id)}
                    onChange={(event) => setTargets(event.currentTarget.checked ? [...targets, option.id] : targets.filter((id) => id !== option.id))}
                  />
                ))}
              </Stack>
            )}
            <Text size="sm" c="dimmed">{t('backupTargetsSelected', { count: targets.length })}</Text>
            <Group>
              <Tooltip label={targets.length === 0 ? t('backupNeedsTargets') : t('backupNeedsResources')} disabled={targets.length > 0 && resources.length > 0}>
                <span>
                  <Button variant="default" leftSection={<Play size={17} />} loading={deliver.isPending} disabled={targets.length === 0 || resources.length === 0} onClick={() => deliver.mutate()}>{t('runNow')}</Button>
                </span>
              </Tooltip>
            </Group>
          </Stack>
        </Paper>

        <Paper withBorder p="lg">
          <Stack gap="md">
            <Title order={2}>{t('restoreBackup')}</Title>
            <Input.Wrapper label={t('restoreFile')} description={t('restoreFileHint')} error={fileError}>
              <Input
                component="input"
                type="file"
                accept=".json,application/json"
                aria-label={t('restoreFile')}
                onChange={(event) => void pickFile(event.currentTarget.files?.[0] ?? null)}
              />
            </Input.Wrapper>
            {artifact && (
              <Alert variant="light" color={artifact.project_id === project.id ? 'pangolin' : 'yellow'} radius="lg" icon={artifact.project_id === project.id ? <DatabaseBackup /> : <AlertTriangle />}>
                <Text size="sm">{t('restoreArtifactSummary', { id: artifact.id, created: formatDate(artifact.created_at) })}</Text>
                <Text size="sm">{t('restoreArtifactResources', { count: artifact.resources.length })}</Text>
                {artifact.project_id !== project.id && <Text size="sm">{t('restoreForeignArtifact', { project: artifact.project_id })}</Text>}
                {/* The artifact is the only place the API names its selectable tables, so it can seed an export. */}
                <Group mt={4}>
                  <Button variant="default" size="compact-sm" onClick={() => setResources(artifact.resources.filter((table) => ALL_BACKUP_TABLES.includes(table)))}>{t('useArtifactResources')}</Button>
                </Group>
              </Alert>
            )}
            <SelectField label={t('conflictStrategy')} value={strategy} onValueChange={setStrategy} options={['fail', 'skip', 'overwrite'].map((value) => ({ value, label: t(`strategy_${value}`) }))} />
            <Group>
              <Tooltip label={restoreDisabledReason ?? ''} disabled={!restoreDisabledReason}>
                <span>
                  <Button color="red" variant="default" leftSection={<History size={17} />} loading={restore.isPending} disabled={Boolean(restoreDisabledReason)} onClick={confirmRestore}>{t('restore')}</Button>
                </span>
              </Tooltip>
            </Group>
          </Stack>
        </Paper>

        <Paper withBorder p="lg">
          <Stack gap="md">
            <div>
              <Title order={2}>{t('backupRuns')}</Title>
              <Text size="sm" c="dimmed">{t('backupRunsHint')}</Text>
            </div>
            {runs.isError || results.isError
              ? <QueryError retry={() => { void runs.refetch(); void results.refetch() }} />
              : runs.isLoading || results.isLoading ? <SkeletonRows />
                : backupRuns.length === 0 ? <EmptyState icon={<History />} title={t('backupRuns')} copy={t('backupRunsEmpty')} />
                  : (
                    <TableScrollContainer minWidth={900}>
                      <Table highlightOnHover>
                        <Table.Thead>
                          <Table.Tr>
                            <Table.Th>{t('createdAt')}</Table.Th>
                            <Table.Th>{t('status')}</Table.Th>
                            <Table.Th>{t('attempts')}</Table.Th>
                            <Table.Th>{t('backupRunTargets')}</Table.Th>
                            <Table.Th><span className="sr-only">{t('actions')}</span></Table.Th>
                          </Table.Tr>
                        </Table.Thead>
                        <Table.Tbody>
                          {backupRuns.map((job) => (
                            <Table.Tr key={job.id}>
                              <Table.Td>{formatDate(job.created_at)}</Table.Td>
                              <Table.Td>{t(jobStatusKey(job.status))}</Table.Td>
                              <Table.Td>{job.attempts}</Table.Td>
                              <Table.Td>
                                <Stack gap={2}>
                                  {(resultsByJob.get(job.id) || []).map((result) => (
                                    <Stack key={result.id} gap={2}>
                                      <Text size="sm" c={result.status === 'succeeded' ? undefined : 'dimmed'} style={{ overflowWrap: 'anywhere' }}>
                                        {t('backupRunTargetLine', { target: targetName(result.storage_id), status: t(jobStatusKey(result.status)), size: result.byte_size == null ? '—' : `${result.byte_size} B`, error: result.error_code || '—' })}
                                      </Text>
                                      {/* Where the archive actually landed. A target the
                                          projection has not recorded a key for reads —. */}
                                      <Text size="sm" c="dimmed" className="mono-cell" style={{ overflowWrap: 'anywhere' }}>
                                        {t('backupRunLocation', { key: result.object_key || '—' })}
                                      </Text>
                                    </Stack>
                                  ))}
                                  {(resultsByJob.get(job.id) || []).length === 0 && <Text size="sm" c="dimmed">{t('backupRunNoTargets')}</Text>}
                                </Stack>
                              </Table.Td>
                              <Table.Td>
                                <Group justify="flex-end">
                                  <Button variant="subtle" size="compact-sm" leftSection={<RotateCcw size={15} />} loading={retry.isPending && retry.variables === job.id} onClick={() => retry.mutate(job.id)}>{t('retry')}</Button>
                                </Group>
                              </Table.Td>
                            </Table.Tr>
                          ))}
                        </Table.Tbody>
                      </Table>
                    </TableScrollContainer>
                  )}
            <Text size="sm" c="dimmed">{t('backupRunsWindow')}</Text>
          </Stack>
        </Paper>

        {/* Automatic delivery and retention schedules stay the shared resource
            editor: they are declarative rows, not a workflow. The retention
            count and its storage target are fields of the payload the API reads,
            so they get inputs instead of asking for hand-written JSON. The
            target list is an auxiliary lookup: a failed read is reported above
            the table rather than in the dialog, because three of the four kinds
            do not need a target and must stay creatable. */}
        <ResourcePage resource="schedules" title={t('backupSchedules')} description={t('backupScheduleHint')} empty={t('backupEmpty')} createLabel={t('addSchedule')} notice={storage.isError ? <InlineQueryError message={t('optionsUnavailable')} onRetry={() => void storage.refetch()} /> : undefined} columns={[{ key: 'kind', label: t('type') }, { key: 'interval_secs', label: t('interval') }, { key: 'next_run_at', label: t('nextRun'), render: formatDate }, { key: 'enabled', label: t('status'), render: (value) => <EnabledPill enabled={value} /> }, { key: 'last_error', label: t('lastError'), render: (value) => value ? <Badge variant="light" color="red">{errorLabel(String(value))}</Badge> : '—' }, { key: 'revision', label: t('revision') }]} fields={[{ key: 'kind', label: t('type'), kind: 'select', options: [{ value: 'automatic_backup', label: t('automaticBackup') }, { value: 'backup_retention', label: t('backupRetention') }, { value: 'probe', label: t('probe') }, { value: 'quota', label: t('quota') }] }, { key: 'storage_id', label: t('storageTarget'), hint: t('backupRetentionStorageHint'), kind: 'select', omitWhenBlank: true, options: storageOptions, fromRow: (row) => payloadOf(row).storage_id }, { key: 'keep', label: t('backupRetentionCount'), hint: t('backupRetentionCountHint'), kind: 'number', omitWhenBlank: true, fromRow: (row) => payloadOf(row).keep }, { key: 'payload', label: t('configuration'), kind: 'json', defaultValue: { targets: [], resources: [] } }, { key: 'interval_secs', label: t('intervalSeconds'), kind: 'number', hint: t('scheduleIntervalRange'), defaultValue: 3600 }, { key: 'enabled', label: t('enabled'), kind: 'checkbox' }, { key: 'revision', label: t('revision'), kind: 'number', defaultValue: 0 }]} normalize={(values, editing) => scheduleBody(values, editing, t)} />
      </Stack>
    </>
  )
}

/** Job status values are the ones `operation_jobs.status` allows. */
function jobStatusKey(status: string): string {
  return ({ pending: 'statusPending', running: 'statusRunning', succeeded: 'statusSucceeded', failed: 'statusFailed' } as Record<string, string>)[status] || 'statusUnknown'
}

type WebhookDelivery = { id: string; webhook_id: string; event_type: string; attempt: number; status: string; response_status: number | null; next_attempt_at: number | null }

/** The statuses `webhook_deliveries.status` records, in the console's words. */
const DELIVERY_STATUS: Record<string, { key: string; color: string }> = {
  pending: { key: 'statusPending', color: 'gray' },
  succeeded: { key: 'statusSucceeded', color: 'teal' },
  failed: { key: 'statusFailed', color: 'red' },
}

/**
 * The delivery history of this project's webhooks. `webhook_deliveries` is the
 * only record of whether an event reached a webhook, and the projection carries
 * the attempt, its status, the upstream response status and the next attempt —
 * no creation or finish timestamp and no error text, so the table does not
 * invent them.
 */
function WebhookDeliveries() {
  const { t } = useTranslation()
  const { project } = useProject()
  const [offset, setOffset] = useState(0)
  const limit = 25
  const query = useQuery({ queryKey: ['webhook-deliveries', project.id, offset], queryFn: () => api<Paged<WebhookDelivery>>(`${projectOperationPath(project.id, 'webhook-deliveries')}?offset=${offset}&limit=${limit}`) })
  // An attempt names its webhook by id; the webhook list is what turns that back
  // into the name the operator configured. Without it the id is shown as-is
  // rather than an empty cell.
  const hooks = useQuery({ queryKey: ['webhook-names', project.id], queryFn: () => api<Paged<Document>>(`${projectOperationPath(project.id, 'webhooks')}?limit=500`) })
  const rows = query.data?.data || []
  const total = query.data?.total ?? rows.length
  const webhookName = (id: string) => String((hooks.data?.data || []).find((row) => row.id === id)?.name ?? id)
  return (
    <Paper withBorder p="lg" mt="lg">
      <Stack gap="md">
        <div>
          <Title order={2}>{t('webhookDeliveries')}</Title>
          <Text size="sm" c="dimmed">{t('webhookDeliveriesHint')}</Text>
        </div>
        {hooks.isError && <InlineQueryError message={t('webhookNamesUnavailable')} onRetry={() => void hooks.refetch()} />}
        {query.isError
          ? <QueryError retry={() => void query.refetch()} />
          : query.isLoading
            ? <SkeletonRows />
            : rows.length === 0
              ? <EmptyState icon={<Webhook />} title={t('webhookDeliveries')} copy={t('webhookDeliveriesEmpty')} />
              : (
                <>
                  <TableScrollContainer minWidth={760} role="region" aria-label={t('webhookDeliveries')} tabIndex={0}>
                    <Table highlightOnHover>
                      <Table.Thead>
                        <Table.Tr>
                          <Table.Th>{t('webhook')}</Table.Th>
                          <Table.Th>{t('events')}</Table.Th>
                          <Table.Th>{t('attempts')}</Table.Th>
                          <Table.Th>{t('status')}</Table.Th>
                          <Table.Th>{t('responseStatus')}</Table.Th>
                          <Table.Th>{t('nextAttempt')}</Table.Th>
                        </Table.Tr>
                      </Table.Thead>
                      <Table.Tbody>
                        {rows.map((delivery) => {
                          const state = DELIVERY_STATUS[delivery.status]
                          return (
                            <Table.Tr key={delivery.id}>
                              <Table.Td>{webhookName(delivery.webhook_id)}</Table.Td>
                              <Table.Td className="mono-cell">{delivery.event_type}</Table.Td>
                              <Table.Td>{delivery.attempt}</Table.Td>
                              <Table.Td><Badge variant="light" color={state?.color ?? 'gray'}>{state ? t(state.key) : t('statusUnknown')}</Badge></Table.Td>
                              {/* The HTTP status the attempt got back. An attempt whose
                                  response the projection did not record is unmeasured,
                                  so it reads — rather than 0. */}
                              <Table.Td className="mono-cell">{delivery.response_status ?? '—'}</Table.Td>
                              <Table.Td>{formatDate(delivery.next_attempt_at)}</Table.Td>
                            </Table.Tr>
                          )
                        })}
                      </Table.Tbody>
                    </Table>
                  </TableScrollContainer>
                  {total > limit && (
                    <Group justify="flex-end" gap="sm" className="pagination">
                      <Pagination total={Math.max(1, Math.ceil(total / limit))} value={Math.floor(offset / limit) + 1} onChange={(page) => setOffset((page - 1) * limit)} getControlProps={(control) => {
                        if (control === 'previous') return { 'aria-label': t('previous') }
                        if (control === 'next') return { 'aria-label': t('next') }
                        return {}
                      }} />
                      <Text size="sm" c="dimmed">{offset + 1}–{Math.min(offset + limit, total)} / {total}</Text>
                    </Group>
                  )}
                </>
              )}
      </Stack>
    </Paper>
  )
}

function jobKindLabel(kind: string, t: (key: string, options?: Record<string, unknown>) => string): string {
  const key = ({ probe: 'probe', quota: 'quota', backup: 'jobKindBackup', automatic_backup: 'automaticBackup', backup_retention: 'backupRetention', gc: 'jobKindGc', catalog_refresh: 'jobKindCatalogRefresh', webhook: 'jobKindWebhook' } as Record<string, string>)[kind]
  return key ? t(key) : kind.replaceAll('_', ' ')
}

const JOB_STATUSES = ['pending', 'running', 'succeeded', 'failed'] as const
/** Kinds whose row action can enqueue a new job of the same work. */
const QUEUEABLE_KINDS = ['probe', 'quota'] as const

/**
 * The durable job list. Leases and fences stay authoritative, so nothing here
 * writes a status: a row action either asks the API to queue a new job or is not
 * offered at all.
 */
function JobsPanel() {
  const errorLabel = useErrorCodeLabel()
  const { t, i18n } = useTranslation()
  const { project } = useProject()
  const client = useQueryClient()
  const [interval, setInterval] = useAutoRefreshInterval('pangolin-auto-refresh-jobs')
  const [status, setStatus] = useState('')
  const [offset, setOffset] = useState(0)
  const [queueKind, setQueueKind] = useState<string | null>(null)
  const [channel, setChannel] = useState('')
  const limit = 25
  const path = projectOperationPath(project.id, 'jobs')
  const params = new URLSearchParams({ offset: String(offset), limit: String(limit) })
  // The API filters a resource with `q`; the row document carries the status, so
  // the filter and the counts below are the server's own view of the same rows.
  if (status) params.set('q', status)
  const query = useQuery({
    queryKey: ['jobs', project.id, status, offset],
    queryFn: () => api<Paged<JobRow>>(`${path}?${params}`),
    refetchInterval: interval ?? false,
  })
  const counts = useQuery({
    queryKey: ['job-counts', project.id],
    queryFn: async () => {
      const totals = await Promise.all(JOB_STATUSES.map(async (value) => [value, (await api<Paged<JobRow>>(`${path}?q=${value}&limit=1`)).total ?? 0] as const))
      return Object.fromEntries(totals) as Record<string, number>
    },
    refetchInterval: interval ?? false,
  })
    // Only the queue dialog needs the channel list, so it loads when that opens.
  const channels = useQuery({ queryKey: ['channel-options', project.id], queryFn: () => api<Paged<Document>>(`${projectOperationPath(project.id, 'channels')}?limit=500`), enabled: queueKind !== null })
  const invalidateJobs = () => { void client.invalidateQueries({ queryKey: ['jobs', project.id] }); void client.invalidateQueries({ queryKey: ['job-counts', project.id] }) }
  const enqueue = useMutation({
    mutationFn: (input: { kind: string; provider_id: string }) => api(projectOperationPath(project.id, input.kind), { method: 'POST', body: JSON.stringify({ provider_id: input.provider_id }) }),
    onSuccess: () => { toast.success(t('jobQueued')); setQueueKind(null); invalidateJobs() },
    onError: (error: Error) => toast.error(error.message),
  })
  const retry = useMutation({
    mutationFn: (job: string) => api<{ id: string }>(`/api/admin/v1/projects/${project.id}/backup/${job}/retry`, { method: 'POST', body: '{}' }),
    onSuccess: (job) => { toast.success(t('backupRetryQueued', { id: job.id })); invalidateJobs() },
    onError: (error: Error) => toast.error(error.message),
  })
  const rows = useMemo(() => {
    const rank = (row: JobRow) => row.status === 'pending' ? 0 : row.status === 'running' ? 1 : 2
    return [...(query.data?.data || [])].sort((a, b) => {
      if (rank(a) !== rank(b)) return rank(a) - rank(b)
      // Waiting work is ordered by when it is due; settled work by recency.
      return rank(a) === 0 ? a.due_at - b.due_at : b.created_at - a.created_at
    })
  }, [query.data])
  const total = query.data?.total ?? rows.length
  const totalPages = Math.max(1, Math.ceil(total / limit))
  const currentPage = Math.floor(offset / limit) + 1
  const channelOptions = (channels.data?.data || []).map((row) => ({ value: String(row.id), label: String(row.name) }))
  const filter = (value: string) => { setStatus(value); setOffset(0) }
  const closeQueue = () => setQueueKind(null)
  return (
    <>
      <PageHeader title={t('jobs')} description={`${t('jobsDescription')} · ${t('updatedAt', { time: formatUpdatedAt(query.dataUpdatedAt, i18n.language) })}`} />
      <Group justify="space-between" align="flex-end" gap="md" wrap="wrap" mb="md">
        <Select
          label={t('status')}
          value={status}
          onChange={(value) => filter(value ?? '')}
          data={[{ value: '', label: t('jobStatusAll') }, ...JOB_STATUSES.map((value) => ({ value, label: t(jobStatusKey(value)) }))]}
          allowDeselect={false}
          w={{ base: '100%', sm: 200 }}
        />
        <AutoRefreshControl interval={interval} onIntervalChange={setInterval} onRefresh={() => Promise.all([query.refetch(), counts.refetch()])} />
      </Group>
      <Group gap="xs" mb="md" wrap="wrap">
        {JOB_STATUSES.map((value) => (
          <Button
            key={value}
            size="compact-sm"
            variant={status === value ? 'light' : 'default'}
            aria-pressed={status === value}
            aria-label={t('jobCountLabel', { status: t(jobStatusKey(value)), count: counts.data?.[value] ?? '—' })}
            onClick={() => filter(status === value ? '' : value)}
          >
            {t('jobCountLabel', { status: t(jobStatusKey(value)), count: counts.data?.[value] ?? '—' })}
          </Button>
        ))}
        {counts.isError && <ActionIcon variant="subtle" color="red" aria-label={t('retry')} onClick={() => void counts.refetch()}><RotateCcw size={16} /></ActionIcon>}
      </Group>
      {query.isError ? <QueryError retry={() => void query.refetch()} /> : query.isLoading ? <SkeletonRows /> : rows.length === 0 ? (
        <EmptyState
          icon={<History />}
          title={t('jobs')}
          copy={status ? t('noSearchResults') : t('jobEmpty')}
          action={status ? <Button variant="default" onClick={() => filter('')}>{t('clearFilters')}</Button> : undefined}
        />
      ) : (
        <TableScrollContainer minWidth={900}>
          <Table stickyHeader highlightOnHover>
            <Table.Thead>
              <Table.Tr>
                <Table.Th>{t('type')}</Table.Th>
                <Table.Th>{t('status')}</Table.Th>
                <Table.Th>{t('attempts')}</Table.Th>
                <Table.Th>{t('dueAt')}</Table.Th>
                <Table.Th>{t('createdAt')}</Table.Th>
                <Table.Th>{t('lastError')}</Table.Th>
                <Table.Th><span className="sr-only">{t('actions')}</span></Table.Th>
              </Table.Tr>
            </Table.Thead>
            <Table.Tbody>
              {rows.map((row) => (
                <Table.Tr key={row.id}>
                  <Table.Td>{jobKindLabel(row.kind, t)}</Table.Td>
                  <Table.Td>{t(jobStatusKey(row.status))}</Table.Td>
                  <Table.Td>{row.attempts}</Table.Td>
                  <Table.Td>{formatDate(row.due_at)}</Table.Td>
                  <Table.Td>{formatDate(row.created_at)}</Table.Td>
                  <Table.Td>{row.error_code ? errorLabel(String(row.error_code)) : '—'}</Table.Td>
                  <Table.Td>
                    <Group justify="flex-end" gap={4} wrap="nowrap">
                      {row.kind === 'backup' && (
                        <Button variant="subtle" size="compact-sm" leftSection={<RotateCcw size={15} />} loading={retry.isPending && retry.variables === row.id} onClick={() => retry.mutate(row.id)}>{t('retry')}</Button>
                      )}
                      {QUEUEABLE_KINDS.includes(row.kind as typeof QUEUEABLE_KINDS[number]) && (
                        <Button variant="subtle" size="compact-sm" leftSection={<Play size={15} />} onClick={() => { setChannel(''); setQueueKind(row.kind) }}>{t('queueNewJob')}</Button>
                      )}
                    </Group>
                  </Table.Td>
                </Table.Tr>
              ))}
            </Table.Tbody>
          </Table>
        </TableScrollContainer>
      )}
      {total > limit && (
        <Group justify="flex-end" gap="sm" mt="md" className="pagination">
          <Pagination total={totalPages} value={currentPage} onChange={(page) => setOffset((page - 1) * limit)} getControlProps={(control) => {
            if (control === 'previous') return { 'aria-label': t('previous') }
            if (control === 'next') return { 'aria-label': t('next') }
            return {}
          }} />
          <Text size="sm" c="dimmed">{offset + 1}–{Math.min(offset + limit, total)} / {total}</Text>
        </Group>
      )}
      <Modal opened={Boolean(queueKind)} onClose={closeQueue} title={queueKind ? t('queueNewJobTitle', { kind: jobKindLabel(queueKind, t) }) : ''} closeButtonProps={{ 'aria-label': t('close') }}>
        <Stack gap="md">
          <Text size="sm" c="dimmed">{t('queueNewJobHint')}</Text>
          {channels.isError ? <QueryError retry={() => void channels.refetch()} /> : channels.isLoading ? <SkeletonRows count={1} /> : channelOptions.length === 0 ? <Text size="sm" c="dimmed">{t('channelEmpty')}</Text> : (
            <SelectField label={t('channel')} value={channel || channelOptions[0]?.value || ''} onValueChange={setChannel} options={channelOptions} />
          )}
          <Group justify="flex-end" gap="xs">
            <Button variant="default" onClick={closeQueue}>{t('cancel')}</Button>
            <Button
              loading={enqueue.isPending}
              disabled={!queueKind || (!channel && channelOptions.length === 0)}
              onClick={() => queueKind && enqueue.mutate({ kind: queueKind, provider_id: channel || channelOptions[0]?.value || '' })}
            >
              {t('queueNewJob')}
            </Button>
          </Group>
        </Stack>
      </Modal>
    </>
  )
}

type InstanceArtifact = { version: number; id: string; created_at: number; schema_digest: string; database_digest: string; include_history: boolean; key_mode: string; salt: string | null; envelope: string }
type InstancePreflight = { artifact_id: string; schema_version: number; tables: Record<string, number>; destination_has_data: boolean; portable: boolean; active_sessions_restored: boolean; derived_available: boolean }

/**
 * Whole-instance disaster recovery. Owner-only on the server, so the panel is
 * only rendered for the owner scope. Restore is destructive and irreversible, so
 * the button stays disabled until a preflight of exactly this archive,
 * passphrase and mode has succeeded — the preflight is what the operator reads
 * before deciding, and a changed input invalidates it.
 */
function InstanceBackupPanel() {
  const { t, i18n } = useTranslation()
  const [includeHistory, setIncludeHistory] = useState(false)
  const [passphrase, setPassphrase] = useState('')
  const [file, setFile] = useState<File | null>(null)
  const [artifact, setArtifact] = useState<InstanceArtifact | null>(null)
  const [fileError, setFileError] = useState<string | null>(null)
  const [mode, setMode] = useState('fail')
  const [force, setForce] = useState(false)
  const [preflight, setPreflight] = useState<{ signature: string; result: InstancePreflight } | null>(null)
  const [restored, setRestored] = useState(false)
  const signature = JSON.stringify({ id: artifact?.id ?? null, passphrase, mode, force })
  const body = () => ({ artifact, passphrase: passphrase || null, mode, force })

  const create = useMutation({
    mutationFn: () => api<InstanceArtifact>('/api/admin/v1/instance/backup', { method: 'POST', body: JSON.stringify({ include_history: includeHistory, passphrase: passphrase || null }) }),
    onSuccess: (created) => {
      downloadJson(`pangolin-instance-${created.id.slice(0, 8)}.json`, created)
      toast.success(t('instanceBackupExported'), { description: t('instanceBackupExportedHint', { key: created.key_mode }) })
    },
    onError: (error: Error) => toast.error(error.message),
  })
  const runPreflight = useMutation({
    mutationFn: () => api<InstancePreflight>('/api/admin/v1/instance/restore/preflight', { method: 'POST', body: JSON.stringify(body()) }),
    onSuccess: (result) => setPreflight({ signature, result }),
    onError: (error: Error) => { setPreflight(null); toast.error(error.message) },
  })
  const restore = useMutation({
    mutationFn: () => api<InstancePreflight>('/api/admin/v1/instance/restore', { method: 'POST', body: JSON.stringify(body()) }),
    onSuccess: () => { setRestored(true); toast.success(t('instanceRestored')) },
    onError: (error: Error) => toast.error(error.message),
  })
  const pickFile = async (picked: File | null) => {
    setFile(picked)
    setArtifact(null)
    setPreflight(null)
    setFileError(null)
    if (!picked) return
    try {
      const parsed: unknown = JSON.parse(await picked.text())
      const candidate = parsed && typeof parsed === 'object' && !Array.isArray(parsed) ? parsed as Partial<InstanceArtifact> : null
      if (!candidate || typeof candidate.id !== 'string' || typeof candidate.envelope !== 'string' || typeof candidate.database_digest !== 'string') {
        setFileError(t('restoreFileInvalid'))
        return
      }
      setArtifact({ version: Number(candidate.version) || 1, id: candidate.id, created_at: Number(candidate.created_at) || 0, schema_digest: String(candidate.schema_digest ?? ''), database_digest: candidate.database_digest, include_history: Boolean(candidate.include_history), key_mode: String(candidate.key_mode ?? 'unknown'), salt: candidate.salt ?? null, envelope: candidate.envelope })
    } catch {
      setFileError(t('restoreFileInvalid'))
    }
  }
  const ready = Boolean(preflight && preflight.signature === signature)
  const confirmRestore = () => {
    if (!ready) return
    confirmAction({
      title: t('instanceRestoreConfirmTitle'),
      body: t('instanceRestoreConfirmBody', { mode: t(mode === 'replace' ? 'instanceModeReplace' : 'instanceModeFail') }),
      confirmLabel: t('instanceRestore'),
      onConfirm: () => restore.mutateAsync(),
    })
  }
  const preflightDisabledReason = !artifact
    ? t('restoreNeedsFile')
    : restore.isPending ? t('instanceRestoreBusy') : null
  return (
    <Paper withBorder p="lg" mb="lg">
      <Stack gap="lg">
        <div>
          <Title order={2}>{t('instanceBackup')}</Title>
          <Text size="sm" c="dimmed">{t('instanceBackupHint')}</Text>
        </div>
        {restored && (
          <Alert variant="light" color="yellow" radius="lg" icon={<ShieldAlert />} title={t('instanceRestored')}>
            <Stack gap="xs">
              <Text size="sm">{t('instanceRestoredHint')}</Text>
              <Group>
                <Button variant="default" onClick={() => window.location.assign('/login')}>{t('instanceRestoreSignIn')}</Button>
              </Group>
            </Stack>
          </Alert>
        )}
        <Stack gap="md">
          <Title order={3}>{t('instanceBackupCreate')}</Title>
          <Switch checked={includeHistory} onChange={(event) => setIncludeHistory(event.currentTarget.checked)} label={t('includeHistory')} description={t('includeHistoryHint')} />
          <SecretInput label={t('passphrase')} description={t('passphraseHint')} value={passphrase} onChange={(event) => setPassphrase(event.currentTarget.value)} autoComplete="new-password" w={{ base: '100%', sm: 320 }} />
          <Group>
            <Button leftSection={<Download size={17} />} loading={create.isPending} onClick={() => create.mutate()}>{t('instanceBackupExportAction')}</Button>
          </Group>
        </Stack>
        <Stack gap="md">
          <Title order={3}>{t('instanceRestore')}</Title>
          <Text size="sm" c="dimmed">{t('instanceRestoreHint')}</Text>
          <Input.Wrapper label={t('instanceRestoreFile')} description={t('instanceRestoreFileHint')} error={fileError}>
            <Input
              component="input"
              type="file"
              accept=".json,application/json"
              aria-label={t('instanceRestoreFile')}
              onChange={(event) => void pickFile(event.currentTarget.files?.[0] ?? null)}
            />
          </Input.Wrapper>
          {artifact && <Text size="sm" c="dimmed">{t('restoreArtifactSummary', { id: artifact.id, created: formatDate(artifact.created_at) })}</Text>}
          <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="md">
            <SelectField label={t('instanceMode')} value={mode} onValueChange={setMode} options={[{ value: 'fail', label: t('instanceModeFail') }, { value: 'replace', label: t('instanceModeReplace') }]} />
            <Stack justify="flex-end">
              <Switch checked={force} onChange={(event) => setForce(event.currentTarget.checked)} label={t('instanceForce')} description={t('instanceForceHint')} />
            </Stack>
          </SimpleGrid>
          <Group>
            <Tooltip label={preflightDisabledReason ?? ''} disabled={!preflightDisabledReason}>
              <span>
                <Button variant="default" loading={runPreflight.isPending} disabled={!artifact} onClick={() => runPreflight.mutate()}>{t('preflight')}</Button>
              </span>
            </Tooltip>
            <Tooltip label={ready ? '' : t('preflightRequired')} disabled={ready}>
              <span>
                <Button color="red" leftSection={<History size={17} />} loading={restore.isPending} disabled={!ready} onClick={confirmRestore}>{t('instanceRestoreAction')}</Button>
              </span>
            </Tooltip>
          </Group>
          {preflight && (
            <Paper withBorder p="md">
              <Stack gap="xs">
                <Title order={4}>{t('preflightResult')}</Title>
                <Text size="sm">{t('preflightArtifact', { id: preflight.result.artifact_id, schema: preflight.result.schema_version })}</Text>
                <Text size="sm">{t('preflightPortable', { value: preflight.result.portable ? t('yes') : t('no'), mode: preflight.result.portable ? t('portableArchive') : t('masterKeyArchive') })}</Text>
                <Text size="sm">{t('preflightDestination', { value: preflight.result.destination_has_data ? t('yes') : t('no') })}</Text>
                <Text size="sm">{t('preflightSessions', { value: preflight.result.active_sessions_restored ? t('yes') : t('no') })}</Text>
                {!preflight.result.derived_available && <Text size="sm" c="dimmed">{t('instanceRestoreDegraded')}</Text>}
                {preflight.result.destination_has_data && mode === 'fail' && <Alert variant="light" color="yellow" radius="lg">{t('preflightReplaceHint')}</Alert>}
                <Text size="sm" fw={600}>{t('preflightTables', { count: Object.keys(preflight.result.tables).length })}</Text>
                <Code block style={{ maxHeight: 220, overflow: 'auto' }}>
                  {Object.entries(preflight.result.tables).sort(([left], [right]) => left.localeCompare(right)).map(([table, count]) => `${table} ${count}`).join('\n')}
                </Code>
              </Stack>
            </Paper>
          )}
        </Stack>
      </Stack>
    </Paper>
  )
}
