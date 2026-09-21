import { Alert, Button, Checkbox, Group, Paper, Select, SimpleGrid, Stack, Switch, Tabs, Text, Textarea, TextInput, Title } from '@mantine/core'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { AlertTriangle, Copy } from 'lucide-react'
import { useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { api, type Bootstrap } from '../api'
import { EnabledPill, SelectField, SkeletonRows } from '../components'
import { palettes } from '../palettes'
import { useProject } from '../project'
import { resolveSkin, skinLabel, skins } from '../skins'
import { useTheme, type ColorMode } from '../theme'
import { PageHeader, QueryError, ResourcePage, displayValue } from './shared'

const SYSTEM_TABS = ['appearance', 'orchestrationSettings', 'requestLogging', 'storage', 'backups', 'webhooks', 'jobs', 'retention', 'about'] as const

export default function SystemPage() {
  const { t } = useTranslation()
  return (
    <Tabs keepMounted={false} defaultValue="appearance">
      <Tabs.List mb="lg">
        {SYSTEM_TABS.map((value) => (
          <Tabs.Tab key={value} value={value}>{t(value)}</Tabs.Tab>
        ))}
      </Tabs.List>
      <Tabs.Panel value="appearance"><Appearance /><BrandingSettings /></Tabs.Panel>
      <Tabs.Panel value="orchestrationSettings"><OrchestrationSettings /></Tabs.Panel>
      <Tabs.Panel value="requestLogging"><LoggingPolicy /></Tabs.Panel>
      <Tabs.Panel value="storage">
        <ResourcePage resource="storage" title={t('storage')} description={t('storageDescription')} empty={t('storageEmpty')} createLabel={t('addStorage')} columns={[{ key: 'name', label: t('name') }, { key: 'kind', label: t('type') }, { key: 'config', label: t('configuration') }, { key: 'revision', label: t('revision') }, { key: 'enabled', label: t('status'), render: (value) => <EnabledPill enabled={value} /> }]} fields={[{ key: 'name', label: t('name'), required: true }, { key: 'config', label: t('configuration'), kind: 'json', defaultValue: { kind: 'local', directory: 'backups' } }, { key: 'secret', label: t('storageSecret'), kind: 'json', omitWhenBlank: true }, { key: 'enabled', label: t('enabled'), kind: 'checkbox' }, { key: 'revision', label: t('revision'), kind: 'number', defaultValue: 0 }]} />
      </Tabs.Panel>
      <Tabs.Panel value="backups"><BackupPanel /></Tabs.Panel>
      <Tabs.Panel value="webhooks">
        <ResourcePage resource="webhooks" title={t('webhooks')} description={t('webhooksDescription')} empty={t('webhookEmpty')} createLabel={t('addWebhook')} columns={[{ key: 'name', label: t('name') }, { key: 'url', label: 'URL', mono: true }, { key: 'subscriptions', label: t('events') }, { key: 'enabled', label: t('status'), render: (value) => <EnabledPill enabled={value} /> }]} fields={[{ key: 'name', label: t('name'), required: true }, { key: 'url', label: 'URL', required: true }, { key: 'events', sourceKey: 'subscriptions', label: t('events'), kind: 'json', defaultValue: ['channel.disabled', 'quota.exhausted', 'request.failed'] }, { key: 'headers', label: t('publicHeaders'), kind: 'json', defaultValue: {} }, { key: 'secret_headers', label: t('secretHeaders'), kind: 'json', omitWhenBlank: true }, { key: 'body', label: t('bodyTemplate'), kind: 'json', defaultValue: '$event' }, { key: 'enabled', label: t('enabled'), kind: 'checkbox' }]} />
      </Tabs.Panel>
      <Tabs.Panel value="jobs">
        <ResourcePage resource="jobs" title={t('jobs')} description={t('jobsDescription')} empty={t('jobEmpty')} immutable columns={[{ key: 'kind', label: t('type') }, { key: 'status', label: t('status') }, { key: 'attempts', label: t('attempts') }, { key: 'due_at', label: t('dueAt') }, { key: 'error_code', label: t('lastError') }]} />
      </Tabs.Panel>
      <Tabs.Panel value="retention">
        <ResourcePage resource="retention" title={t('retention')} description={t('retentionDescription')} empty={t('retentionEmpty')} createLabel={t('addPolicy')} columns={[{ key: 'resource_type', label: t('type') }, { key: 'retention_days', label: t('retentionDays') }, { key: 'retain_payloads', label: t('retainPayloads') }]} fields={[{ key: 'resource_type', label: t('type'), kind: 'select', options: ['requests', 'payloads', 'probes', 'quota'].map((value) => ({ value, label: t(value) })) }, { key: 'retention_days', label: t('retentionDays'), kind: 'number', defaultValue: 30 }, { key: 'retain_payloads', label: t('retainPayloads'), kind: 'checkbox' }]} />
      </Tabs.Panel>
      <Tabs.Panel value="about"><About /></Tabs.Panel>
    </Tabs>
  )
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
  if (query.isError) return <QueryError retry={() => void query.refetch()} />
  if (query.isLoading || !query.data) return <SkeletonRows count={2} />
  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const data = new FormData(event.currentTarget);
    save.mutate({ instance_name: String(data.get('instance_name')), branding_name: String(data.get('branding_name')), favicon_url: String(data.get('favicon_url')), onboarding_complete: data.get('onboarding_complete') === 'on' })
  }
  return (
    <Paper withBorder p="lg" mt="lg">
      <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="xl">
        <Stack gap="xs">
          <Title order={2}>{t('branding')}</Title>
          <Text size="sm" c="dimmed">{t('brandingHint')}</Text>
        </Stack>
        <form onSubmit={submit}>
          <Stack gap="md">
            <TextInput name="instance_name" label={t('instanceName')} defaultValue={query.data.instance_name} required />
            <TextInput name="branding_name" label={t('brandingName')} defaultValue={query.data.branding_name} required />
            <TextInput name="favicon_url" label={t('faviconUrl')} defaultValue={query.data.favicon_url} required />
            <Checkbox name="onboarding_complete" label={t('onboardingComplete')} defaultChecked={query.data.onboarding_complete} />
            <Button type="submit">{t('save')}</Button>
          </Stack>
        </form>
      </SimpleGrid>
    </Paper>
  )
}

type OrchestrationDocument = { version: number; affinity_rules: unknown[]; session_compaction: Record<string, unknown> }
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
      save.mutate({ version: 1, affinity_rules: JSON.parse(String(data.get('affinity_rules'))), session_compaction: JSON.parse(String(data.get('session_compaction'))) })
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

function BackupPanel() {
  const { t } = useTranslation()
  const { project } = useProject()
  const [artifact, setArtifact] = useState('')
  const base = `/api/admin/v1/projects/${project.id}/backup`
  const run = useMutation({ mutationFn: ({ path, body }: { path: string; body: unknown }) => api<unknown>(`${base}/${path}`, { method: 'POST', body: JSON.stringify(body) }), onSuccess: (value) => { setArtifact(JSON.stringify(value, null, 2)); toast.success(t('saved')) }, onError: (error: Error) => toast.error(error.message) })
  const resources = (value: FormDataEntryValue | null) => String(value || '').split(',').map((item) => item.trim()).filter(Boolean)
  const exportNow = (event: FormEvent<HTMLFormElement>) => { event.preventDefault(); const data = new FormData(event.currentTarget); run.mutate({ path: 'export', body: { resources: resources(data.get('resources')) } }) }
  const restore = (event: FormEvent<HTMLFormElement>) => { event.preventDefault(); const data = new FormData(event.currentTarget); try { run.mutate({ path: 'restore', body: { artifact: JSON.parse(String(data.get('artifact'))), strategy: data.get('strategy') } }) } catch { toast.error(t('invalidJson')) } }
  const deliver = (event: FormEvent<HTMLFormElement>) => { event.preventDefault(); const data = new FormData(event.currentTarget); run.mutate({ path: 'run', body: { targets: resources(data.get('targets')), resources: resources(data.get('resources')) } }) }
  return (
    <>
      <PageHeader title={t('backups')} description={t('backupsDescription')} />
      <SimpleGrid cols={{ base: 1, md: 3 }} spacing="md" mb="xl">
        <Paper withBorder p="lg">
          <form onSubmit={exportNow}>
            <Stack gap="md">
              <Title order={2}>{t('exportBackup')}</Title>
              <TextInput name="resources" label={t('resources')} description={t('resourcesHint')} required />
              <Button type="submit">{t('export')}</Button>
            </Stack>
          </form>
        </Paper>
        <Paper withBorder p="lg">
          <form onSubmit={deliver}>
            <Stack gap="md">
              <Title order={2}>{t('deliverBackup')}</Title>
              <TextInput name="targets" label={t('targetIds')} required />
              <TextInput name="resources" label={t('resources')} required />
              <Button type="submit">{t('runNow')}</Button>
            </Stack>
          </form>
        </Paper>
        <Paper withBorder p="lg">
          <form onSubmit={restore}>
            <Stack gap="md">
              <Title order={2}>{t('restoreBackup')}</Title>
              <Textarea name="artifact" label={t('artifactJson')} required defaultValue={artifact} />
              <Select name="strategy" label={t('conflictStrategy')} defaultValue="fail" data={[{ value: 'fail', label: 'fail' }, { value: 'skip', label: 'skip' }, { value: 'overwrite', label: 'overwrite' }]} />
              <Button type="submit">{t('restore')}</Button>
            </Stack>
          </form>
        </Paper>
      </SimpleGrid>
      <ResourcePage resource="schedules" title={t('backupSchedules')} description={t('backupScheduleHint')} empty={t('backupEmpty')} createLabel={t('addSchedule')} columns={[{ key: 'kind', label: t('type') }, { key: 'interval_secs', label: t('interval') }, { key: 'next_run_at', label: t('nextRun') }, { key: 'enabled', label: t('status'), render: (value) => <EnabledPill enabled={value} /> }, { key: 'revision', label: t('revision') }]} fields={[{ key: 'kind', label: t('type'), kind: 'select', options: [{ value: 'automatic_backup', label: t('automaticBackup') }, { value: 'backup_retention', label: t('backupRetention') }, { value: 'probe', label: t('probe') }, { value: 'quota', label: t('quota') }] }, { key: 'payload', label: t('configuration'), kind: 'json', defaultValue: { targets: [], resources: [] } }, { key: 'interval_secs', label: t('intervalSeconds'), kind: 'number', defaultValue: 3600 }, { key: 'enabled', label: t('enabled'), kind: 'checkbox' }, { key: 'revision', label: t('revision'), kind: 'number', defaultValue: 0 }]} />
    </>
  )
}