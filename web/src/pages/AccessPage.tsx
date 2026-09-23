import { Accordion, ActionIcon, Badge, Button, Checkbox, Code, Group, NumberInput, Paper, Select, SimpleGrid, Stack, Tabs, Table, TableScrollContainer, Text, TextInput, Textarea, Title } from '@mantine/core'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Archive, BarChart3, KeyRound, Link2Off, Mail, Plus, RotateCw, Trash2, UserMinus } from 'lucide-react'
import { useEffect, useMemo, useRef, useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { api, type AnalyticsRow, type Document } from '../api'
import { confirmAction, EmptyState, EnabledPill, InlineQueryError, Modal, SecretInput, SelectField, SkeletonRows } from '../components'
import { AllowedModelsEditor, ProfileMappingsEditor, RoutingPolicyEditor } from './documentEditors'
import { UNMEASURED, formatCount, formatMicros } from '../observability'
import { projectOperationPath, useProject } from '../project'
import { PageHeader, QueryError, ResourcePage, displayValue, formatDate } from './shared'

const ACCESS_TABS: Array<[string, string]> = [['keys', 'api_key:manage'], ['profiles', 'api_key:manage'], ['projects', 'project:manage'], ['users', 'user:manage'], ['members', 'project:read'], ['roles', 'role:manage'], ['invitations', 'project:manage'], ['oidc', 'oidc:manage'], ['identities', 'oidc:manage']]

/**
 * The API-key type the record system stores, in the console's own words. A type
 * this build does not know is shown verbatim: it is data, not copy.
 */
const KEY_TYPE_KEYS: Record<string, string> = { service: 'keyTypeService', user: 'keyTypeUser', personal: 'keyTypePersonal', no_auth: 'keyTypeNoAuth' }
const KEY_TYPES = ['user', 'service', 'personal', 'no_auth'] as const
// The complete `level='project'` seed set used by the API-key form. Role editing
// uses the actor-filtered server catalog below; key writes still enforce this
// independently on the server.
const PROJECT_SCOPES = [
  ['project:manage', 'scopeProjectManage'],
  ['project:read', 'scopeProjectRead'],
  ['gateway:use', 'scopeGatewayUse'],
  ['role:manage', 'scopeRoleManage'],
  ['api_key:manage', 'scopeApiKeyManage'],
] as const
const PERMISSION_DESCRIPTION_KEYS = new Map<string, string>(PROJECT_SCOPES)
type PermissionCatalogEntry = { slug: string; level: string; description: string }

function useKeyTypeLabel() {
  const { t } = useTranslation()
  return (type: unknown) => {
    const key = KEY_TYPE_KEYS[String(type)]
    return key ? t(key) : displayValue(type)
  }
}

export default function AccessPage() {
  const { t } = useTranslation()
  const { project, permissions, permissionsStatus, retryPermissions } = useProject()
  const [tab, setTab] = useState('keys')
  // Mirrors access::authorize: project:manage satisfies project:read, role:manage
  // and api_key:manage. Without this the UI hid tabs the backend would accept.
  const granted = new Set<string>(permissions)
  if (granted.has('project:manage')) for (const implied of ['project:read', 'role:manage', 'api_key:manage']) granted.add(implied)
  const can = (permission: string) => granted.has('*') || granted.has(permission)
  const visible = ACCESS_TABS.filter(([, permission]) => can(permission))
  const active = visible.some(([v]) => v === tab) ? tab : visible[0]?.[0]
  // The active project's permissions are what this page may offer, and the query is keyed
  // by the project, so a switch leaves them unanswered for the project on screen. That is
  // not "no access": while the answer is in flight the page says which project's
  // permissions it is reading and keeps the panel the operator was working in mounted —
  // hidden and unreachable — so a switch cannot discard their work or state a denial it
  // has not been told. `tab` is the operator's own last choice, never a permission, and
  // nothing in the hidden region is offered, read as an authorization claim or reachable
  // by pointer or assistive tech, so project B is granted no control early. Actions are
  // unchanged: every panel still acts through the same server-checked calls.
  const loading = permissionsStatus === 'loading'
  const mounted = active ?? (ACCESS_TABS.some(([value]) => value === tab) ? tab : undefined)
  const header = <PageHeader title={t('access')} description={t('keysDescription')} />
  // An answer that could not be read is not a denial either, and it has its own retry.
  if (permissionsStatus === 'error') return <div>{header}<QueryError retry={retryPermissions} /></div>
  // Only an answer that grants no module says so.
  if (!active && !loading) return <div>{header}<Paper withBorder p="lg" mt="md"><Text size="sm" c="dimmed">{t('noAccessSection')}</Text></Paper></div>
  return (
    <div>
      {header}
      {loading && (
        <Stack gap="sm" mt="md">
          <Text size="sm" c="dimmed">{t('accessPermissionsLoading')}</Text>
          <SkeletonRows count={3} />
        </Stack>
      )}
      <div hidden={loading}>
        <Tabs keepMounted={false} value={mounted} onChange={(v) => v && setTab(v)}>
          <Tabs.List mb="lg">
            {visible.map(([value]) => (
              <Tabs.Tab key={value} value={value}>{t(value)}</Tabs.Tab>
            ))}
        </Tabs.List>
        <Tabs.Panel value="keys"><KeysPanel /><KeyLoggingForm /></Tabs.Panel>
        <Tabs.Panel value="profiles"><ProfilesPanel /></Tabs.Panel>
        <Tabs.Panel value="projects">
          <ResourcePage resource="projects" endpoint="/api/admin/v1/projects" itemEndpoint={(id) => `/api/admin/v1/projects/${id}`} updateMethod="PATCH" title={t('projects')} description={t('projectsDescription')} empty={t('projectEmpty')} createLabel={t('addProject')} columns={[{ key: 'name', label: t('name') }, { key: 'slug', label: t('slug'), mono: true }, { key: 'enabled', label: t('status'), render: (value) => <EnabledPill enabled={value} /> }]} fields={[{ key: 'name', label: t('name'), required: true }, { key: 'slug', label: t('slug'), required: true }, { key: 'owner_user_id', label: t('ownerId') }, { key: 'enabled', label: t('enabled'), kind: 'checkbox' }]} />
        </Tabs.Panel>
        <Tabs.Panel value="users">
          <ResourcePage resource="users" endpoint="/api/admin/v1/users" itemEndpoint={(id) => `/api/admin/v1/users/${id}`} updateMethod="PATCH" title={t('users')} description={t('usersDescription')} empty={t('userEmpty')} createLabel={t('addUser')} columns={[{ key: 'display_name', label: t('name') }, { key: 'email', label: t('email') }, { key: 'role', label: t('role') }, { key: 'enabled', label: t('status'), render: (value) => <EnabledPill enabled={value} /> }]} fields={[{ key: 'email', label: t('email'), required: true }, { key: 'display_name', label: t('name') }, { key: 'password', label: t('password'), kind: 'secret', hint: t('passwordHint') }, { key: 'language', label: t('language'), defaultValue: 'zh-CN' }, { key: 'enabled', label: t('enabled'), kind: 'checkbox' }]} />
        </Tabs.Panel>
        <Tabs.Panel value="members"><MembersPanel canWrite={can('project:manage')} /></Tabs.Panel>
        <Tabs.Panel value="roles">
          <RolesPanel key={project.id} />
          <AssignmentsPanel />
        </Tabs.Panel>
        <Tabs.Panel value="invitations"><InvitationsPanel /></Tabs.Panel>
        <Tabs.Panel value="oidc">
          <ResourcePage
            resource="oidc"
            endpoint="/api/admin/v1/oidc/providers"
            itemEndpoint={(id) => `/api/admin/v1/oidc/providers/${id}`}
            updateMethod="PATCH"
            title="OIDC"
            description={t('oidcDescription')}
            empty={t('oidcEmpty')}
            createLabel={t('addOidc')}
            columns={[
              { key: 'display_name', label: t('displayName') },
              { key: 'name', label: t('name') },
              { key: 'login_only', label: t('oidcLoginOnly') },
              { key: 'issuer_url', label: t('issuer'), mono: true },
              { key: 'client_id', label: t('clientId'), mono: true },
              { key: 'enabled', label: t('status'), render: (value) => <EnabledPill enabled={value} /> },
              { key: 'secret_configured', label: t('secretConfigured') },
            ]}
            fields={[
              { key: 'name', label: t('name'), required: true },
              { key: 'display_name', label: t('displayName'), hint: t('oidcDisplayNameHint'), required: true, validate: (value) => value.trim().length > 80 ? t('oidcDisplayNameInvalid') : undefined },
              { key: 'button_color', label: t('oidcButtonColor'), hint: t('oidcButtonColorHint'), validate: (value) => value && !/^#[0-9a-f]{6}$/i.test(value) ? t('oidcButtonColorInvalid') : undefined },
              { key: 'logo_key', label: t('oidcLogoKey'), hint: t('oidcLogoKeyHint'), validate: (value) => value && (value.length > 128 || !/^[A-Za-z0-9_-]+:[A-Za-z0-9_-]+$/.test(value)) ? t('oidcLogoKeyInvalid') : undefined },
              { key: 'login_only', label: t('oidcLoginOnly'), hint: t('oidcLoginOnlyHint'), kind: 'checkbox', defaultValue: false },
              { key: 'issuer_url', label: t('issuer'), required: true },
              { key: 'client_id', label: t('clientId'), required: true },
              { key: 'client_secret', label: t('clientSecret'), kind: 'secret' },
              { key: 'scopes', label: t('scopes'), kind: 'json', defaultValue: ['openid', 'profile', 'email'] },
              { key: 'claim_mapping', label: t('claimMapping'), kind: 'json', defaultValue: { version: 1, jit: true, email_claim: 'email', name_claim: 'name', groups_claim: 'groups' } },
              { key: 'enabled', label: t('enabled'), kind: 'checkbox' },
            ]}
          />
        </Tabs.Panel>
        <Tabs.Panel value="identities"><OidcIdentitiesPanel /></Tabs.Panel>
        </Tabs>
      </div>
    </div>
  )
}

function RolesPanel() {
  const { t } = useTranslation()
  const { project } = useProject()
  const catalog = useQuery({
    queryKey: ['permission-catalog', project.id],
    queryFn: () => api<PermissionCatalogEntry[]>(`/api/admin/v1/projects/${encodeURIComponent(project.id)}/permission-catalog`),
  })
  const permissions = (catalog.data ?? []).map((permission) => ({
    ...permission,
    description: PERMISSION_DESCRIPTION_KEYS.has(permission.slug) ? t(PERMISSION_DESCRIPTION_KEYS.get(permission.slug)!) : permission.description,
  }))
  return <ResourcePage
    resource="roles"
    canDelete={(row) => !row.is_system}
    editDisabled={(row) => Boolean(row.is_system)}
    endpoint={(projectId) => `/api/admin/v1/projects/${projectId}/roles`}
    itemEndpoint={(id, projectId) => `/api/admin/v1/projects/${projectId}/roles/${id}`}
    updateMethod="PATCH"
    title={t('roles')}
    description={t('rolesDescription')}
    empty={t('roleEmpty')}
    createLabel={t('addRole')}
    columns={[{ key: 'name', label: t('name') }, { key: 'scope', label: t('scope') }, { key: 'is_system', label: t('systemRole') }]}
    fields={[
      { key: 'name', label: t('name'), required: true },
      {
        key: 'permissions',
        label: t('permissions'),
        hint: t('permissionPickerHint'),
        kind: 'permission-list',
        defaultValue: ['project:read'],
        permissions,
        loading: catalog.isLoading,
        error: catalog.isError ? t('permissionCatalogUnavailable') : undefined,
        onRetry: () => void catalog.refetch(),
        emptyMessage: t('permissionCatalogEmpty'),
      },
    ]}
  />
}

type ProfileDocument = Document & { name: string; rpm_limit: number | null; tpm_limit: number | null; budget_micros: number | null; routing_policy: unknown; mappings: unknown[]; allowed_models: unknown[] }
type ProfileTemplate = { id: string; name: string; profile: { version: number; rpm_limit: number | null; tpm_limit: number | null; budget_micros: number | null; routing_policy: unknown; mappings: unknown[]; allowed_models: unknown[] } }

function ProfilesPanel() {
  const { t } = useTranslation()
  const { project } = useProject()
  const client = useQueryClient()
  const activeProject = useRef(project.id)
  activeProject.current = project.id
  const [saveSource, setSaveSource] = useState<ProfileDocument | null>(null)
  const [applyTemplate, setApplyTemplate] = useState<ProfileTemplate | null>(null)
  const [applyTarget, setApplyTarget] = useState('')
  const [formProjectId, setFormProjectId] = useState('')
  const [importOpen, setImportOpen] = useState(false)
  const [importText, setImportText] = useState('')
  const [importConflict, setImportConflict] = useState('fail')
  const [formError, setFormError] = useState<string | null>(null)
  const [exportText, setExportText] = useState<string | null>(null)
  const [exportingId, setExportingId] = useState<string | null>(null)
  const exportRequest = useRef(0)
  useEffect(() => {
    exportRequest.current += 1
    setExportingId(null)
    setExportText(null)
  }, [project.id])
  const templatePath = `/api/admin/v1/projects/${project.id}/profile-templates`
  const templates = useQuery({ queryKey: ['profile-templates', project.id], queryFn: () => api<{ data: ProfileTemplate[]; total: number }>(`${templatePath}?limit=500`) })
  const profiles = useQuery({ queryKey: ['profile-template-targets', project.id], queryFn: () => api<{ data: ProfileDocument[] }>(`${projectOperationPath(project.id, 'key-profiles')}?limit=500`) })
  const profileOptions = (profiles.data?.data || []).map((item) => ({ value: item.id, label: item.name }))
  const changedProject = Boolean(formProjectId) && formProjectId !== project.id
  const refresh = (projectId: string) => {
    void client.invalidateQueries({ queryKey: ['profile-templates', projectId] })
    void client.invalidateQueries({ queryKey: ['resource', projectId, 'key-profiles'] })
    void client.invalidateQueries({ queryKey: ['profile-template-targets', projectId] })
  }
  const saveTemplate = useMutation({
    mutationFn: ({ projectId, sourceId, name }: { projectId: string; sourceId: string; name: string }) => api(`/api/admin/v1/projects/${projectId}/profile-templates`, { method: 'POST', body: JSON.stringify({ name, source_profile_id: sourceId }) }),
    onSuccess: (_result, variables) => { toast.success(t('profileTemplateSaved')); setSaveSource(null); refresh(variables.projectId) },
    onError: (error: Error) => setFormError(error.message),
  })
  const apply = useMutation({
    mutationFn: ({ projectId, templateId, targetId }: { projectId: string; templateId: string; targetId: string }) => api(`/api/admin/v1/projects/${projectId}/profile-templates/${templateId}/apply`, { method: 'POST', body: JSON.stringify({ target_profile_id: targetId }) }),
    onSuccess: (_result, variables) => { toast.success(t('profileTemplateApplied')); refresh(variables.projectId) },
  })
  const importTemplate = useMutation({
    mutationFn: ({ projectId, document, conflict }: { projectId: string; document: unknown; conflict: string }) => api(`/api/admin/v1/projects/${projectId}/profile-templates/import`, { method: 'POST', body: JSON.stringify({ document, conflict }) }),
    onSuccess: (_result, variables) => { toast.success(t('profileTemplateImported')); setImportOpen(false); setImportText(''); refresh(variables.projectId) },
    onError: (error: Error) => setFormError(error.message),
  })
  const remove = useMutation({
    mutationFn: ({ projectId, templateId }: { projectId: string; templateId: string }) => api(`/api/admin/v1/projects/${projectId}/profile-templates/${templateId}`, { method: 'DELETE' }),
    onSuccess: (_result, variables) => refresh(variables.projectId),
  })
  const beginSave = (row: Document) => { setSaveSource(row as ProfileDocument); setFormProjectId(project.id); setFormError(null) }
  const beginApply = (template: ProfileTemplate) => { setApplyTemplate(template); setApplyTarget(profileOptions[0]?.value || ''); setFormProjectId(project.id); setFormError(null) }
  const submitSave = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    if (!saveSource || changedProject) return setFormError(t('profileTemplateProjectChanged'))
    const name = String(new FormData(event.currentTarget).get('name') || '').trim()
    saveTemplate.mutate({ projectId: formProjectId, sourceId: saveSource.id, name })
  }
  const reviewApply = () => {
    if (!applyTemplate || !applyTarget || changedProject) return setFormError(t('profileTemplateProjectChanged'))
    const projectId = formProjectId
    const templateId = applyTemplate.id
    const targetId = applyTarget
    const templateName = applyTemplate.name
    const targetName = profileOptions.find((option) => option.value === targetId)?.label || targetId
    setApplyTemplate(null)
    confirmAction({
      title: t('profileTemplateApplyTitle', { template: templateName, profile: targetName }),
      body: t('profileTemplateApplyBody'),
      confirmLabel: t('profileTemplateApply'),
      onConfirm: () => {
        if (activeProject.current !== projectId) throw new Error(t('profileTemplateProjectChanged'))
        return apply.mutateAsync({ projectId, templateId, targetId })
      },
    })
  }
  const submitImport = () => {
    setFormError(null)
    if (changedProject) return setFormError(t('profileTemplateProjectChanged'))
    try {
      const document = JSON.parse(importText) as { name?: unknown }
      if (typeof document.name !== 'string' || !document.name.trim()) return setFormError(t('profileTemplateInvalidJson'))
      const projectId = formProjectId
      const conflict = importConflict
      setImportOpen(false)
      confirmAction({
        title: t('profileTemplateImportTitle', { name: document.name }),
        body: t('profileTemplateImportBody', { strategy: t(`strategy_${conflict}`) }),
        confirmLabel: t('profileTemplateImport'),
        onConfirm: () => {
          if (activeProject.current !== projectId) throw new Error(t('profileTemplateProjectChanged'))
          return importTemplate.mutateAsync({ projectId, document, conflict })
        },
      })
    }
    catch { setFormError(t('profileTemplateInvalidJson')) }
  }
  const exportTemplate = async (template: ProfileTemplate) => {
    const projectId = project.id
    const requestId = ++exportRequest.current
    setExportingId(template.id)
    try {
      const document = await api<unknown>(`/api/admin/v1/projects/${projectId}/profile-templates/${template.id}/export`)
      if (requestId !== exportRequest.current || activeProject.current !== projectId) return
      setExportText(JSON.stringify(document, null, 2))
    } catch (error) {
      if (requestId === exportRequest.current && activeProject.current === projectId) toast.error(error instanceof Error ? error.message : String(error))
    }
    finally { if (requestId === exportRequest.current) setExportingId(null) }
  }
  return <Stack gap="xl">
    <ResourcePage
      resource="key-profiles"
      title={t('profiles')}
      description={t('profilesDescription')}
      empty={t('profileEmpty')}
      createLabel={t('addProfile')}
      columns={[{ key: 'name', label: t('name') }, { key: 'rpm_limit', label: 'RPM' }, { key: 'tpm_limit', label: 'TPM' }, { key: 'budget_micros', label: t('budget') }, { key: 'routing_policy', label: t('routingPolicy') }]}
      fields={[{ key: 'name', label: t('name'), required: true }, { key: 'rpm_limit', label: 'RPM', kind: 'number' }, { key: 'tpm_limit', label: 'TPM', kind: 'number' }, { key: 'budget_micros', label: t('budgetMicros'), kind: 'number' }, { key: 'routing_policy', label: t('routingPolicy'), kind: 'json', defaultValue: { version: 1 }, render: ({ name, label, value, setValid }) => <RoutingPolicyEditor name={name} label={label} value={value} setValid={setValid} /> }, { key: 'mappings', label: t('modelMappings'), kind: 'json', defaultValue: [], render: ({ name, label, value, setValid }) => <ProfileMappingsEditor name={name} label={label} value={value} setValid={setValid} /> }, { key: 'allowed_models', label: t('allowedModels'), kind: 'json', defaultValue: [], render: ({ name, label, value, setValid }) => <AllowedModelsEditor name={name} label={label} value={value} setValid={setValid} /> }]}
      rowActions={(row) => <Button variant="subtle" size="compact-sm" onClick={() => beginSave(row)}>{t('profileTemplateSaveAs')}</Button>}
    />
    <section aria-labelledby="profile-templates-heading">
      <Group justify="space-between" align="flex-start" mb="md" wrap="wrap">
        <Stack gap={0}><Title id="profile-templates-heading" order={2}>{t('profileTemplates')}</Title><Text size="sm" c="dimmed">{t('profileTemplatesDescription')}</Text></Stack>
        <Button variant="default" onClick={() => { setImportOpen(true); setFormProjectId(project.id); setFormError(null) }}>{t('profileTemplateImport')}</Button>
      </Group>
      {templates.isLoading ? <SkeletonRows count={2} /> : templates.isError ? <InlineQueryError message={t('profileTemplatesUnavailable')} onRetry={() => void templates.refetch()} /> : !templates.data?.data.length ? <EmptyState icon={<KeyRound />} title={t('profileTemplates')} copy={t('profileTemplatesEmpty')} /> : <Stack gap="sm">{templates.data.data.map((template) => <Paper key={template.id} withBorder p="md"><Group justify="space-between" align="flex-start" wrap="wrap"><Stack gap={4}><Text fw={600}>{template.name}</Text><Text size="sm" c="dimmed">{t('profileTemplateSummary', { rpm: template.profile.rpm_limit ?? '—', tpm: template.profile.tpm_limit ?? '—' })}</Text></Stack><Group gap="xs"><Button variant="default" size="compact-sm" aria-label={t('profileTemplateApplyNamed', { name: template.name })} onClick={() => beginApply(template)}>{t('profileTemplateApply')}</Button><Button variant="subtle" size="compact-sm" loading={exportingId === template.id} aria-label={t('profileTemplateExportNamed', { name: template.name })} onClick={() => void exportTemplate(template)}>{t('profileTemplateExport')}</Button><ActionIcon variant="subtle" color="red" aria-label={t('profileTemplateDeleteNamed', { name: template.name })} onClick={() => { const projectId = project.id; confirmAction({ title: t('profileTemplateDeleteTitle', { name: template.name }), body: t('profileTemplateDeleteBody'), onConfirm: () => { if (activeProject.current !== projectId) throw new Error(t('profileTemplateProjectChanged')); return remove.mutateAsync({ projectId, templateId: template.id }) } }) }}><Trash2 size={16} /></ActionIcon></Group></Group></Paper>)}</Stack>}
    </section>
    <Modal open={Boolean(saveSource)} onOpenChange={(open) => { if (!open) setSaveSource(null) }} title={t('profileTemplateSaveTitle', { name: saveSource?.name || '' })}>
      <form onSubmit={submitSave}><Stack gap="md"><TextInput name="name" label={t('profileTemplateName')} required autoFocus />{changedProject && <Text role="alert" size="sm" c="red">{t('profileTemplateProjectChanged')}</Text>}{formError && <Text role="alert" size="sm" c="red">{formError}</Text>}<Group justify="flex-end"><Button variant="default" type="button" onClick={() => setSaveSource(null)}>{t('cancel')}</Button><Button type="submit" loading={saveTemplate.isPending} disabled={changedProject}>{t('profileTemplateSave')}</Button></Group></Stack></form>
    </Modal>
    <Modal open={Boolean(applyTemplate)} onOpenChange={(open) => { if (!open) setApplyTemplate(null) }} title={t('profileTemplateApplyPreview')} description={t('profileTemplateApplyPreviewHint')}>
      <Stack gap="md">{profiles.isLoading ? <SkeletonRows count={1} /> : profiles.isError ? <InlineQueryError message={t('profileTargetsUnavailable')} onRetry={() => void profiles.refetch()} /> : profileOptions.length === 0 ? <Text size="sm" c="dimmed">{t('profileTargetsEmpty')}</Text> : <SelectField label={t('profileTemplateTarget')} value={applyTarget} onValueChange={setApplyTarget} options={profileOptions} />}<Code block>{applyTemplate ? `${applyTemplate.profile.rpm_limit ?? '—'} RPM · ${applyTemplate.profile.tpm_limit ?? '—'} TPM\n${JSON.stringify(applyTemplate.profile, null, 2)}` : ''}</Code>{changedProject && <Text role="alert" size="sm" c="red">{t('profileTemplateProjectChanged')}</Text>}{formError && <Text role="alert" size="sm" c="red">{formError}</Text>}<Group justify="flex-end"><Button variant="default" onClick={() => setApplyTemplate(null)}>{t('cancel')}</Button><Button onClick={reviewApply} disabled={!applyTarget || profiles.isError || changedProject}>{t('profileTemplateReviewApply')}</Button></Group></Stack>
    </Modal>
    <Modal open={importOpen} onOpenChange={setImportOpen} title={t('profileTemplateImport')} description={t('profileTemplateImportHint')}><Stack gap="md"><Textarea label={t('profileTemplateJson')} value={importText} onChange={(event) => setImportText(event.target.value)} rows={12} /><SelectField label={t('conflictStrategy')} value={importConflict} onValueChange={setImportConflict} options={[{ value: 'fail', label: t('strategy_fail') }, { value: 'overwrite', label: t('strategy_overwrite') }]} />{changedProject && <Text role="alert" size="sm" c="red">{t('profileTemplateProjectChanged')}</Text>}{formError && <Text role="alert" size="sm" c="red">{formError}</Text>}<Group justify="flex-end"><Button variant="default" onClick={() => setImportOpen(false)}>{t('cancel')}</Button><Button onClick={submitImport} loading={importTemplate.isPending} disabled={!importText.trim() || changedProject}>{t('profileTemplateImport')}</Button></Group></Stack></Modal>
    <Modal open={exportText !== null} onOpenChange={(open) => { if (!open) setExportText(null) }} title={t('profileTemplateExport')}><Stack gap="md"><Textarea aria-label={t('profileTemplateJson')} value={exportText || ''} readOnly rows={14} /><Group justify="flex-end"><Button onClick={() => setExportText(null)}>{t('close')}</Button></Group></Stack></Modal>
  </Stack>
}

type ScopedKey = { id: string; name: string; key_prefix: string; scopes: string[]; budget_micros: number | null; spent_micros: number | null; enabled: boolean; key_type: string; profile_id: string | null; expires_at: number | null; last_used_at: number | null; allowed_ips_json: string; denied_ips_json: string; lifecycle?: 'active' | 'archived'; archived_at?: number | null }

/**
 * The state admission actually enforces (`src/db.rs::authenticate_api_key`): a
 * disabled key, one whose `expires_at` has passed, one whose spend has reached
 * its budget, or a key that authenticates. A field the response does not carry
 * is treated the way the record system treats NULL — as no constraint — rather
 * than guessed at, so a row from an older response shape is never called expired.
 */
export type KeyState = 'archived' | 'disabled' | 'expired' | 'exhausted' | 'usable'
export function keyState(key: Pick<ScopedKey, 'enabled' | 'expires_at' | 'budget_micros' | 'spent_micros' | 'lifecycle'>, now = Math.floor(Date.now() / 1000)): KeyState {
  if (key.lifecycle === 'archived') return 'archived'
  if (!key.enabled) return 'disabled'
  if (typeof key.expires_at === 'number' && key.expires_at <= now) return 'expired'
  if (typeof key.budget_micros === 'number' && typeof key.spent_micros === 'number' && key.spent_micros >= key.budget_micros) return 'exhausted'
  return 'usable'
}

type KeyUsageWindow = 'today' | 'last7d' | 'all'

function keyUsageBounds(window: KeyUsageWindow, now = new Date()) {
  const until = Math.floor(now.getTime() / 1000) + 1
  if (window === 'all') return { from: 0, until }
  const start = new Date(now.getFullYear(), now.getMonth(), now.getDate())
  if (window === 'last7d') start.setDate(start.getDate() - 7)
  return { from: Math.floor(start.getTime() / 1000), until }
}

function KeyUsageDialog({ projectId, apiKey, onClose }: { projectId: string; apiKey: ScopedKey; onClose: () => void }) {
  const { t } = useTranslation()
  const [window, setWindow] = useState<KeyUsageWindow>('today')
  const bounds = useMemo(() => keyUsageBounds(window), [window])
  const query = useQuery({
    queryKey: ['key-usage', projectId, apiKey.id, window, bounds.from, bounds.until],
    queryFn: () => api<{ data: AnalyticsRow[] }>(`/api/admin/v1/projects/${encodeURIComponent(projectId)}/analytics?dimension=model&api_key=${encodeURIComponent(apiKey.id)}&from=${bounds.from}&until=${bounds.until}`),
  })
  const rows = query.data?.data ?? []
  // Older servers omit the marker and remain compatible; a current false value
  // means requests existed but no authoritative usage row did.
  const measuredRows = rows.filter((row) => row.usage_measured !== false)
  const measured = measuredRows.length > 0
  const total = (field: 'input_tokens' | 'output_tokens' | 'cache_hit_tokens' | 'cost_micros') => measured
    ? measuredRows.reduce((sum, row) => sum + (typeof row[field] === 'number' ? row[field] : 0), 0)
    : null
  const input = total('input_tokens')
  const output = total('output_tokens')
  const cache = total('cache_hit_tokens')
  const cost = total('cost_micros')
  const top = [...rows].sort((left, right) => (right.input_tokens + right.output_tokens) - (left.input_tokens + left.output_tokens)).slice(0, 3)
  const count = (value: number | null) => value == null ? UNMEASURED : formatCount(value)
  return <Modal open onOpenChange={(open) => !open && onClose()} title={t('keyUsageTitle', { name: apiKey.name })}>
    <Stack gap="md">
      <Tabs value={window} onChange={(value) => value && setWindow(value as KeyUsageWindow)}>
        <Tabs.List grow>
          <Tabs.Tab value="today">{t('today')}</Tabs.Tab>
          <Tabs.Tab value="last7d">{t('last7d')}</Tabs.Tab>
          <Tabs.Tab value="all">{t('allRetained')}</Tabs.Tab>
        </Tabs.List>
      </Tabs>
      {query.isError ? <QueryError retry={() => void query.refetch()} /> : query.isLoading ? <SkeletonRows count={4} /> : <>
        {!measured && <Text size="sm" c="dimmed">{t('keyUsageUnmeasured')}</Text>}
        <TableScrollContainer minWidth={440} role="region" aria-label={t('keyUsageOverall')} tabIndex={0}>
          <Table>
            <Table.Thead><Table.Tr><Table.Th>{t('tokenType')}</Table.Th><Table.Th>{t('count')}</Table.Th></Table.Tr></Table.Thead>
            <Table.Tbody>
              <Table.Tr><Table.Td>{t('inputTokens')}</Table.Td><Table.Td className="mono-cell">{count(input)}</Table.Td></Table.Tr>
              <Table.Tr><Table.Td>{t('outputTokens')}</Table.Td><Table.Td className="mono-cell">{count(output)}</Table.Td></Table.Tr>
              <Table.Tr><Table.Td>{t('cacheTokens')}</Table.Td><Table.Td className="mono-cell">{count(cache)}</Table.Td></Table.Tr>
              <Table.Tr><Table.Td>{t('totalTokens')}</Table.Td><Table.Td className="mono-cell">{input == null || output == null ? UNMEASURED : formatCount(input + output)}</Table.Td></Table.Tr>
              <Table.Tr><Table.Td>{t('cost')}</Table.Td><Table.Td className="mono-cell">{cost == null ? UNMEASURED : formatMicros(cost)}</Table.Td></Table.Tr>
            </Table.Tbody>
          </Table>
        </TableScrollContainer>
        {top.length > 0 && <Stack gap="xs">
          <Title order={3}>{t('keyUsageTopModels')}</Title>
          {top.map((row) => <Paper key={row.dimension ?? 'unknown'} withBorder p="sm">
            <Group justify="space-between" align="flex-start" wrap="wrap">
              <Code>{row.dimension || UNMEASURED}</Code>
              <Text size="sm" className="mono-cell">{row.usage_measured === false ? `${UNMEASURED} · ${UNMEASURED}` : `${formatCount(row.input_tokens + row.output_tokens)} ${t('tokens')} · ${formatMicros(row.cost_micros)}`}</Text>
            </Group>
          </Paper>)}
        </Stack>}
      </>}
    </Stack>
  </Modal>
}

/** Each state in the console's words, with the tone it reads as. */
const KEY_STATE_LABELS: Record<KeyState, { key: string; color: string }> = {
  archived: { key: 'archived', color: 'gray' },
  disabled: { key: 'disabled', color: 'gray' },
  expired: { key: 'keyStateExpired', color: 'red' },
  exhausted: { key: 'keyStateExhausted', color: 'red' },
  usable: { key: 'keyStateUsable', color: 'teal' },
}

const keysPath = (projectId: string) => `/api/admin/v1/projects/${projectId}/api-keys`

function KeysPanel() {
  const { t } = useTranslation()
  const keyTypeLabel = useKeyTypeLabel()
  const { project } = useProject()
  const client = useQueryClient()
  const [open, setOpen] = useState(false)
  const [editing, setEditing] = useState<ScopedKey | null>(null)
  const [selected, setSelected] = useState<string[]>([])
  const [mode, setMode] = useState('generated')
  const [keyType, setKeyType] = useState('service')
  const [owner, setOwner] = useState('')
  const [scopes, setScopes] = useState<string[]>(['gateway:use'])
  const [formProjectId, setFormProjectId] = useState('')
  const [ownerError, setOwnerError] = useState<string | null>(null)
  const [scopeError, setScopeError] = useState<string | null>(null)
  const [createError, setCreateError] = useState<string | null>(null)
  const [usageKey, setUsageKey] = useState<{ projectId: string; key: ScopedKey } | null>(null)
  const [token, setToken] = useState<{ value: string; title: string; description: string } | null>(null)
  const [profile, setProfile] = useState('__none__')
  // The profile options and the bulk selection both belong to a project, so both are
  // dropped when the project changes: the selection names keys of the project it was
  // made in, and the panel is kept mounted across a project switch now, so it would
  // otherwise stand over another project's rows and offer to act on ids that are not
  // on screen. Keeping the array identity when it is already empty avoids a re-render.
  useEffect(() => { setProfile('__none__'); setEditProfile('__none__'); setOwner(''); setOwnerError(null); setUsageKey(null); setSelected((current) => current.length ? [] : current) }, [project.id])
  const [editProfile, setEditProfile] = useState('__none__')
  const path = keysPath(project.id)
  const query = useQuery({ queryKey: ['keys', project.id], queryFn: () => api<ScopedKey[]>(path) })
  const profiles = useQuery({ queryKey: ['profile-options', project.id], queryFn: () => api<{ data: Array<{ id: string; name: string }> }>(`/api/admin/v1/projects/${project.id}/operations/key-profiles?limit=500`) })
  const profileOptions = [{ value: '__none__', label: t('noProfile') }, ...(profiles.data?.data || []).map((profile) => ({ value: profile.id, label: profile.name }))]
  const ownerRequired = keyType === 'user' || keyType === 'personal'
  const formProjectChanged = Boolean(formProjectId) && formProjectId !== project.id
  const owners = useQuery({ queryKey: ['key-owner-options', project.id], queryFn: () => api<MembershipRow[]>(`/api/admin/v1/projects/${project.id}/members`), enabled: open && ownerRequired && !formProjectChanged })
  const ownerOptions = (owners.data || []).filter((member) => member.status === 'active').map((member) => ({
    value: member.user_id,
    label: member.display_name ? `${member.display_name}${member.email ? ` (${member.email})` : ''}` : member.email || member.user_id,
  }))
  type CreateKeyMutation = { projectId: string; body: unknown }
  const create = useMutation({
    mutationFn: ({ projectId, body }: CreateKeyMutation) => api<{ key: ScopedKey; token?: string; mode: string }>(keysPath(projectId), { method: 'POST', body: JSON.stringify(body) }),
    onSuccess: (data, variables) => { void client.invalidateQueries({ queryKey: ['keys', variables.projectId] }); setOpen(false); if (data.token) setToken({ value: data.token, title: t('keyCreated'), description: t('keyCreatedHint') }); else toast.success(t('importComplete')) },
    onError: (error: Error) => setCreateError(error.message),
  })
  const update = useMutation({ mutationFn: ({ projectId, id, body }: { projectId: string; id: string; body: unknown }) => api(`${keysPath(projectId)}/${id}`, { method: 'PATCH', body: JSON.stringify(body) }), onSuccess: (_, variables) => { setEditing(null); setEditProfile('__none__'); void client.invalidateQueries({ queryKey: ['keys', variables.projectId] }) }, onError: (error: Error) => toast.error(error.message) })
  type KeyAction = { projectId: string; projectName: string; keyId: string; keyName: string }
  const rotate = useMutation({
    mutationFn: ({ projectId, keyId }: KeyAction) => api<{ key: ScopedKey; token: string }>(`${keysPath(projectId)}/${keyId}/rotate`, { method: 'POST' }),
    onSuccess: async (data, variables) => {
      await client.invalidateQueries({ queryKey: ['keys', variables.projectId] })
      setToken({ value: data.token, title: t('keyRotated'), description: t('keyRotatedHint', { name: variables.keyName, project: variables.projectName }) })
    },
    onError: (error: Error) => toast.error(error.message),
  })
  const archive = useMutation({
    mutationFn: ({ projectId, keyId }: KeyAction) => api<ScopedKey>(`${keysPath(projectId)}/${keyId}/archive`, { method: 'POST' }),
    onSuccess: async (_, variables) => {
      try {
        await client.invalidateQueries({ queryKey: ['keys', variables.projectId] }, { throwOnError: true })
        toast.success(t('saved'))
      } catch {
        toast.error(t('savedListNotRefreshed'))
      }
    },
    onError: (error: Error) => toast.error(error.message),
  })
  const beginCreate = () => {
    setFormProjectId(project.id)
    setKeyType('service')
    setOwner('')
    setScopes(['gateway:use'])
    setOwnerError(null)
    setScopeError(null)
    setCreateError(null)
    setOpen(true)
  }
  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    setCreateError(null)
    setOwnerError(null)
    setScopeError(null)
    if (formProjectChanged) return
    if (ownerRequired && !owner) { setOwnerError(t('keyOwnerRequired')); return }
    if (!scopes.length) { setScopeError(t('keyScopeRequired')); return }
    const data = new FormData(event.currentTarget)
    create.mutate({
      projectId: formProjectId,
      body: {
        name: data.get('name'),
        token_mode: mode,
        token: mode === 'import_existing' ? data.get('token') : null,
        key_type: keyType,
        user_id: ownerRequired ? owner : null,
        scopes,
        profile_id: data.get('profile_id') === '__none__' ? null : data.get('profile_id') || null,
        budget_micros: data.get('budget_micros') ? Number(data.get('budget_micros')) : null,
        expires_at: data.get('expires_at') ? Number(data.get('expires_at')) : null,
        allowed_ips: String(data.get('allowed_ips') || '').split(',').map((v) => v.trim()).filter(Boolean),
        denied_ips: String(data.get('denied_ips') || '').split(',').map((v) => v.trim()).filter(Boolean),
      },
    })
  }
  const submitEdit = (event: FormEvent<HTMLFormElement>) => { event.preventDefault(); if (!editing) return; const data = new FormData(event.currentTarget); update.mutate({ projectId: project.id, id: editing.id, body: { name: data.get('name'), profile_id: data.get('profile_id') === '__none__' ? null : data.get('profile_id') || null, budget_micros: data.get('budget_micros') ? Number(data.get('budget_micros')) : null, expires_at: data.get('expires_at') ? Number(data.get('expires_at')) : null, allowed_ips: String(data.get('allowed_ips') || '').split(',').map((v) => v.trim()).filter(Boolean), denied_ips: String(data.get('denied_ips') || '').split(',').map((v) => v.trim()).filter(Boolean) } }) }
  type BulkKeyAction = { projectId: string; ids: string[]; action: 'enable' | 'disable' | 'archive' }
  const bulk = useMutation({
    mutationFn: ({ projectId, ids, action }: BulkKeyAction) => action === 'archive'
      ? api(`${keysPath(projectId)}/bulk-archive`, { method: 'POST', body: JSON.stringify({ ids }) })
      : api(projectOperationPath(projectId, 'bulk-toggle'), { method: 'POST', body: JSON.stringify({ resource: 'keys', ids, enabled: action === 'enable' }) }),
    // This table renders `['keys', project.id]`, so that is the list the action changed.
    // The exact list is re-read before the selection is dropped, and `throwOnError` is
    // what makes that ordering a guarantee rather than a hope: the mutation stays
    // pending until the refetch settles, so the operator sees the state the server
    // recorded. One click gets one result: the success is said only once the list has
    // been re-read, so it is never contradicted afterwards, and a list that cannot be
    // re-read is reported as the single fact it is — the write landed, the list could
    // not be confirmed — with the selection kept so the operator can retry or reconcile.
    onSuccess: async (_, variables) => {
      try {
        await client.invalidateQueries({ queryKey: ['keys', variables.projectId] }, { throwOnError: true })
        if (project.id === variables.projectId) setSelected([])
        toast.success(t('saved'))
      } catch {
        toast.error(t('savedListNotRefreshed'))
      }
    },
    onError: (error: Error) => toast.error(error.message),
  })
  return (
    <>
      {/* One solid primary per page: with no keys the empty state below owns the
          call to action, so the header only carries a button once a key exists. */}
      <PageHeader title={t('keys')} description={t('keysDescription')} action={query.data?.length ? <Button leftSection={<Plus size={17} />} onClick={beginCreate}>{t('addKey')}</Button> : undefined} />
      {selected.length > 0 && <Group gap="sm" mb="md" role="region" aria-label={t('bulkActions')} className="selection-bar">
        <Text size="sm" fw={540}>{t('selectedCount', { count: selected.length })}</Text>
        <Button size="compact-sm" variant="default" loading={bulk.isPending} onClick={() => bulk.mutate({ projectId: project.id, ids: [...selected], action: 'enable' })}>{t('enable')}</Button>
        <Button size="compact-sm" variant="default" loading={bulk.isPending} onClick={() => bulk.mutate({ projectId: project.id, ids: [...selected], action: 'disable' })}>{t('disable')}</Button>
        <Button size="compact-sm" variant="default" color="red" loading={bulk.isPending} onClick={() => { const action = { projectId: project.id, ids: [...selected], action: 'archive' as const }; confirmAction({ title: t('bulkArchiveKeysTitle', { count: action.ids.length }), body: t('bulkArchiveKeysBody'), confirmLabel: t('archive'), onConfirm: () => bulk.mutateAsync(action) }) }}>{t('archive')}</Button>
        <Button size="compact-sm" variant="subtle" onClick={() => setSelected([])}>{t('cancel')}</Button>
      </Group>}
      {query.isError ? <QueryError retry={() => void query.refetch()} /> : query.isLoading ? <SkeletonRows /> : !query.data?.length ? <EmptyState icon={<KeyRound />} title={t('keys')} copy={t('keyEmpty')} action={<Button onClick={beginCreate}>{t('addKey')}</Button>} /> : (
        <TableScrollContainer minWidth={1020} role="region" aria-label={t('keys')} tabIndex={0}>
          <Table stickyHeader highlightOnHover>
            <Table.Thead>
              <Table.Tr>
                <Table.Th><Checkbox aria-label={t('selectAll')} checked={query.data.some((key) => key.lifecycle !== 'archived') && query.data.filter((key) => key.lifecycle !== 'archived').every((key) => selected.includes(key.id))} onChange={(event) => setSelected(event.currentTarget.checked ? query.data.filter((key) => key.lifecycle !== 'archived').map((key) => key.id) : [])} /></Table.Th>
                <Table.Th>{t('name')}</Table.Th>
                <Table.Th>{t('fingerprint')}</Table.Th>
                <Table.Th>{t('type')}</Table.Th>
                <Table.Th>{t('scopes')}</Table.Th>
                <Table.Th>{t('spendAgainstBudget')}</Table.Th>
                <Table.Th>{t('expiresAt')}</Table.Th>
                <Table.Th>{t('lastUsed')}</Table.Th>
                <Table.Th>{t('ipPolicy')}</Table.Th>
                <Table.Th>{t('status')}</Table.Th>
                <Table.Th><span className="sr-only">{t('actions')}</span></Table.Th>
              </Table.Tr>
            </Table.Thead>
            <Table.Tbody>
              {query.data.map((key) => {
                const state = keyState(key)
                return (
                  <Table.Tr key={key.id}>
                    <Table.Td><Checkbox aria-label={`${t('selectRow')} ${key.name}`} disabled={key.lifecycle === 'archived'} checked={selected.includes(key.id)} onChange={(event) => setSelected((current) => event.currentTarget.checked ? [...current, key.id] : current.filter((id) => id !== key.id))} /></Table.Td>
                    <Table.Td><strong>{key.name}</strong></Table.Td>
                    <Table.Td><Code>{key.key_prefix}</Code></Table.Td>
                    <Table.Td>{keyTypeLabel(key.key_type)}</Table.Td>
                    <Table.Td>{key.scopes?.length ? <Stack gap={2}>{key.scopes.map((scope) => <Code key={scope}>{scope}</Code>)}</Stack> : '—'}</Table.Td>
                    {/* Spend against the budget, both in micro-USD; an absent
                        budget is unmeasured, not zero. */}
                    <Table.Td className="mono-cell">{`${formatMicros(key.spent_micros)} / ${formatMicros(key.budget_micros)}`}</Table.Td>
                    <Table.Td>{formatDate(key.expires_at)}</Table.Td>
                    {/* A key nobody has authenticated with is unmeasured, so it reads —. */}
                    <Table.Td>{formatDate(key.last_used_at)}</Table.Td>
                    <Table.Td><Code>{key.allowed_ips_json === '[]' && key.denied_ips_json === '[]' ? t('anyIp') : t('restricted')}</Code></Table.Td>
                    <Table.Td>
                      <Group gap={6} wrap="nowrap">
                        {/* The state is what admission enforces, so an enabled but
                            expired key never reads as "Enabled". */}
                        <Badge variant="light" color={KEY_STATE_LABELS[state].color}>{t(KEY_STATE_LABELS[state].key)}</Badge>
                        {key.lifecycle !== 'archived' && <Button
                          variant="subtle"
                          size="compact-sm"
                          aria-label={`${key.enabled ? t('disable') : t('enable')} ${key.name}`}
                          onClick={() => update.mutate({ projectId: project.id, id: key.id, body: { enabled: !key.enabled } })}
                        >
                          {key.enabled ? t('disable') : t('enable')}
                        </Button>}
                      </Group>
                    </Table.Td>
                    <Table.Td>
                      <Group gap={4} justify="flex-end" wrap="nowrap">
                        <Button variant="subtle" size="compact-sm" leftSection={<BarChart3 size={15} />} aria-label={`${t('usage')} ${key.name}`} onClick={() => setUsageKey({ projectId: project.id, key })}>{t('usage')}</Button>
                        {key.lifecycle !== 'archived' && <>
                          <Button variant="subtle" size="compact-sm" leftSection={<RotateCw size={15} />} aria-label={`${t('rotate')} ${key.name}`} onClick={() => { const action = { projectId: project.id, projectName: project.name, keyId: key.id, keyName: key.name }; confirmAction({ title: t('rotateKeyTitle', { name: action.keyName }), body: t('rotateKeyBody', { project: action.projectName }), confirmLabel: t('rotate'), onConfirm: () => rotate.mutateAsync(action) }) }}>{t('rotate')}</Button>
                          <Button variant="subtle" size="compact-sm" onClick={() => { setEditing(key); setEditProfile(key.profile_id || '__none__') }}>{t('edit')}</Button>
                          <Button variant="subtle" color="red" size="compact-sm" leftSection={<Archive size={15} />} aria-label={`${t('archive')} ${key.name}`} onClick={() => { const action = { projectId: project.id, projectName: project.name, keyId: key.id, keyName: key.name }; confirmAction({ title: t('archiveKeyTitle', { name: action.keyName }), body: t('archiveKeyBody', { project: action.projectName }), confirmLabel: t('archive'), onConfirm: () => archive.mutateAsync(action) }) }}>{t('archive')}</Button>
                        </>}
                      </Group>
                    </Table.Td>
                  </Table.Tr>
                )
              })}
            </Table.Tbody>
          </Table>
        </TableScrollContainer>
      )}
      <Modal open={open} onOpenChange={setOpen} title={t('addKey')} description={mode === 'import_existing' ? t('importSecurity') : t('generatedSecurity')}>
        <form onSubmit={submit}>
          <Stack gap="md">
            <SelectField label={t('tokenMode')} value={mode} onValueChange={setMode} options={[{ value: 'generated', label: t('generateToken') }, { value: 'import_existing', label: t('importExisting') }]} />
            {mode === 'import_existing' && <SecretInput name="token" label={t('existingToken')} description={t('importTokenHint')} minLength={32} maxLength={1024} autoComplete="off" required />}
            <TextInput name="name" label={t('keyName')} required autoFocus />
            <SelectField label={t('keyType')} value={keyType} onValueChange={(value) => { setKeyType(value); setOwner(''); setOwnerError(null); setCreateError(null) }} options={KEY_TYPES.map((value) => ({ value, label: keyTypeLabel(value) }))} />
            {ownerRequired && (owners.isLoading
              ? <Stack gap={4}><Text size="sm" fw={500}>{t('keyOwner')}</Text><SkeletonRows count={1} /></Stack>
              : owners.isError
                ? <Stack gap={4}><Text size="sm" fw={500}>{t('keyOwner')}</Text><InlineQueryError message={t('keyOwnersUnavailable')} onRetry={() => void owners.refetch()} /></Stack>
                : <>
                    <Select name="user_id" label={t('keyOwner')} placeholder={t('keyOwnerPlaceholder')} value={owner || null} onChange={(value) => { setOwner(value || ''); setOwnerError(null); setCreateError(null) }} data={ownerOptions} allowDeselect={false} disabled={formProjectChanged} />
                    {ownerError && <Text role="alert" size="sm" c="red">{ownerError}</Text>}
                  </>)}
            <Checkbox.Group label={t('keyScopes')} description={t('keyScopesHint')} value={scopes} onChange={(value) => { setScopes(value); setScopeError(null); setCreateError(null) }}>
              <Stack gap="xs" mt="xs">
                {PROJECT_SCOPES.map(([value, label]) => <Checkbox key={value} value={value} label={t(label)} />)}
              </Stack>
            </Checkbox.Group>
            {scopeError && <Text role="alert" size="sm" c="red">{scopeError}</Text>}
            {/* The profile list is a lookup: if it cannot be read, the picker would
                silently offer only "no profile". */}
            {profiles.isError
              ? <InlineQueryError message={t('optionsUnavailable')} onRetry={() => void profiles.refetch()} />
              : <SelectField name="profile_id" label={t('profileId')} value={profile} onValueChange={setProfile} options={profileOptions} />}
            <Group gap="md" grow>
              <NumberInput name="budget_micros" label={t('budgetMicros')} min={0} hideControls />
              <NumberInput name="expires_at" label={t('expiresAtEpoch')} min={1} hideControls />
            </Group>
            <Group gap="md" grow>
              <TextInput name="allowed_ips" label={t('allowedIps')} placeholder="192.0.2.0/24" />
              <TextInput name="denied_ips" label={t('deniedIps')} placeholder="192.0.2.10" />
            </Group>
            {formProjectChanged && <Text role="alert" size="sm" c="red">{t('keyProjectChanged')}</Text>}
            {createError && <Text role="alert" size="sm" c="red">{createError}</Text>}
            <Group justify="flex-end" gap="xs" pt="xs">
              <Button variant="default" type="button" onClick={() => setOpen(false)}>{t('cancel')}</Button>
              <Button type="submit" loading={create.isPending} disabled={formProjectChanged || (ownerRequired && owners.isError)}>{t('save')}</Button>
            </Group>
          </Stack>
        </form>
      </Modal>
      <Modal open={Boolean(editing)} onOpenChange={(value) => !value && setEditing(null)} title={t('editKey')}>
        <form onSubmit={submitEdit}>
          <Stack gap="md">
            <TextInput name="name" label={t('keyName')} required defaultValue={editing?.name} />
            {profiles.isError
              ? <InlineQueryError message={t('optionsUnavailable')} onRetry={() => void profiles.refetch()} />
              : <SelectField name="profile_id" label={t('profileId')} value={editProfile} onValueChange={setEditProfile} options={profileOptions} />}
            <Group gap="md" grow>
              <NumberInput name="budget_micros" label={t('budgetMicros')} min={0} defaultValue={editing?.budget_micros ?? ''} hideControls />
              <NumberInput name="expires_at" label={t('expiresAtEpoch')} min={1} defaultValue={editing?.expires_at ?? ''} hideControls />
            </Group>
            <Group gap="md" grow>
              <TextInput name="allowed_ips" label={t('allowedIps')} defaultValue={editing ? JSON.parse(editing.allowed_ips_json).join(', ') : ''} />
              <TextInput name="denied_ips" label={t('deniedIps')} defaultValue={editing ? JSON.parse(editing.denied_ips_json).join(', ') : ''} />
            </Group>
            <Group justify="flex-end" gap="xs">
              <Button variant="default" type="button" onClick={() => { setEditing(null); setEditProfile('__none__') }}>{t('cancel')}</Button>
              <Button type="submit" loading={update.isPending}>{t('save')}</Button>
            </Group>
          </Stack>
        </form>
      </Modal>
      <Modal open={Boolean(token)} onOpenChange={(value) => !value && setToken(null)} title={token?.title || t('keyCreated')} description={token?.description || t('keyCreatedHint')}>
        <Stack gap="md">
          <Code block>{token?.value}</Code>
          <Button onClick={() => token && navigator.clipboard.writeText(token.value)}>{t('copy')}</Button>
        </Stack>
      </Modal>
      {usageKey && <KeyUsageDialog key={`${usageKey.projectId}:${usageKey.key.id}`} projectId={usageKey.projectId} apiKey={usageKey.key} onClose={() => setUsageKey(null)} />}
    </>
  )
}

function KeyLoggingForm() {
  const { t } = useTranslation()
  const { project } = useProject()
  const [level, setLevel] = useState('inherit')
  // Same query key as the key table above, so this observes the cached list
  // instead of issuing a second request.
  const keys = useQuery({ queryKey: ['keys', project.id], queryFn: () => api<ScopedKey[]>(keysPath(project.id)) })
  const save = useMutation({ mutationFn: (body: unknown) => api(`/api/admin/v1/projects/${project.id}/operations/key-logging`, { method: 'POST', body: JSON.stringify(body) }), onSuccess: () => toast.success(t('saved')), onError: (error: Error) => toast.error(error.message) })
  const submit = (event: FormEvent<HTMLFormElement>) => { event.preventDefault(); save.mutate({ api_key_id: new FormData(event.currentTarget).get('api_key_id'), level }) }
  // Per-key logging has nothing to configure while the project has no keys, and
  // an errored key list already surfaces its own retry above.
  if (!keys.data?.length) return null
  return (
    <Accordion mt="md" variant="contained">
      <Accordion.Item value="key-logging">
        <Accordion.Control>{t('keyLoggingPolicy')}</Accordion.Control>
        <Accordion.Panel>
          <form onSubmit={submit}>
            <Group gap="md" align="end" wrap="wrap">
              <TextInput name="api_key_id" label={t('keyId')} required />
              <SelectField name="level" label={t('requestLogging')} value={level} onValueChange={setLevel} options={['inherit', 'off', 'metadata', 'redacted_body', 'full_body'].map((value) => ({ value, label: t(value) }))} />
              <Button type="submit">{t('save')}</Button>
            </Group>
          </form>
        </Accordion.Panel>
      </Accordion.Item>
    </Accordion>
  )
}

type RoleBindingRow = { id: string; user_id: string; role_id: string; project_id: string | null; created_at: number }

/**
 * Membership and role assignments. A role binding is created, listed and
 * revoked, and the panel used to be a create-only form, so a binding could never
 * be seen or taken back. Listing reads the project-scoped route
 * (`/projects/{project}/users/{user_id}/role-bindings`), which authorises
 * `role:manage` on the project — the same permission creating a binding needs —
 * so a project manager can see and revoke what they can grant.
 */
function AssignmentsPanel() {
  const { t } = useTranslation()
  const { project, permissions } = useProject()
  const client = useQueryClient()
  const canList = permissions.has('*') || permissions.has('role:manage') || permissions.has('user:manage')
  const [bindingUser, setBindingUser] = useState('')
  const [typedUser, setTypedUser] = useState('')
  // A binding belongs to a project, so the project is part of the cache identity: the
  // request path names it, and the same user id in another project has its own rows.
  const bindingsKey = ['role-bindings', project.id, bindingUser] as const
  // The list a mutation must re-read is the one it changed, so the identity is carried
  // in the variables and read back from there: a project switch while the request is in
  // flight must not redirect the invalidation to whatever project is on screen when the
  // response settles.
  type BindingsSnapshot = { projectId: string; userId: string }
  const invalidateBindings = (snapshot: BindingsSnapshot) => void client.invalidateQueries({ queryKey: ['role-bindings', snapshot.projectId, snapshot.userId] })
  const save = useMutation({
    mutationFn: ({ path, body }: { path: string; body: unknown; bindings?: BindingsSnapshot }) => api(path, { method: 'POST', body: JSON.stringify(body) }),
    onSuccess: (_data, variables) => {
      toast.success(t('saved'))
      // A new binding belongs to the user the form named, so that user's list is
      // re-read: the row appears without a second "Show bindings" round-trip.
      if (variables.bindings) invalidateBindings(variables.bindings)
    },
    onError: (error: Error) => toast.error(error.message),
  })
  const roles = useQuery({ queryKey: ['roles', project.id], queryFn: () => api<Array<{ id: string; name: string }>>(`/api/admin/v1/projects/${project.id}/roles`) })
  const bindings = useQuery({
    queryKey: bindingsKey,
    enabled: canList && bindingUser !== '',
    queryFn: () => api<RoleBindingRow[]>(`/api/admin/v1/projects/${project.id}/users/${encodeURIComponent(bindingUser)}/role-bindings`),
  })
  const revoke = useMutation({
    mutationFn: ({ row }: { row: RoleBindingRow; bindings: BindingsSnapshot }) => api(`/api/admin/v1/users/${encodeURIComponent(row.user_id)}/role-bindings/${encodeURIComponent(row.id)}`, { method: 'DELETE' }),
    onSuccess: (_data, variables) => { toast.success(t('bindingRevoked')); invalidateBindings(variables.bindings) },
    onError: (error: Error) => toast.error(error.message),
  })
  const member = (event: FormEvent<HTMLFormElement>) => { event.preventDefault(); const data = new FormData(event.currentTarget); save.mutate({ path: `/api/admin/v1/projects/${project.id}/members`, body: { user_id: data.get('user_id'), role_id: data.get('role_id'), status: data.get('status') } }) }
  const binding = (event: FormEvent<HTMLFormElement>) => { event.preventDefault(); const data = new FormData(event.currentTarget); const user = String(data.get('user_id')); save.mutate({ path: `/api/admin/v1/users/${user}/role-bindings`, body: { role_id: data.get('role_id'), project_id: project.id }, bindings: { projectId: project.id, userId: user } }); setBindingUser(user) }
  const roleName = (id: string) => roles.data?.find((role) => role.id === id)?.name || id
  return (
    <Accordion mt="md" variant="contained">
      <Accordion.Item value="assignments">
        <Accordion.Control>{t('assignments')}</Accordion.Control>
        <Accordion.Panel>
          <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="lg">
            <form onSubmit={member}>
              <Stack gap="md">
                <Title order={3}>{t('projectMembership')}</Title>
                <TextInput name="user_id" label={t('userId')} required />
                <TextInput name="role_id" label={t('roleId')} required />
                <Select name="status" label={t('status')} defaultValue="active" data={[{ value: 'active', label: t('memberStatusActive') }, { value: 'suspended', label: t('memberStatusSuspended') }]} />
                <Button type="submit">{t('save')}</Button>
              </Stack>
            </form>
            <form onSubmit={binding}>
              <Stack gap="md">
                <Title order={3}>{t('roleBinding')}</Title>
                <TextInput name="user_id" label={t('userId')} required value={typedUser} onChange={(event) => setTypedUser(event.currentTarget.value)} />
                <TextInput name="role_id" label={t('roleId')} required />
                <Group gap="xs">
                  <Button type="submit">{t('assign')}</Button>
                  <Button type="button" variant="default" disabled={!canList || typedUser.trim() === ''} onClick={() => setBindingUser(typedUser.trim())}>{t('showBindings')}</Button>
                </Group>
              </Stack>
            </form>
          </SimpleGrid>
          <Stack gap="sm" mt="lg">
            <Title order={3}>{t('roleBindingsList')}</Title>
            {/* The role name is resolved from the project's role list; a lookup
                that could not be read leaves the ids in place and says so. */}
            {bindingUser !== '' && canList && roles.isError && <InlineQueryError message={t('projectRolesUnavailable')} onRetry={() => void roles.refetch()} />}
            {/* Anyone who can see this tab holds role:manage, which is also what
                listing needs, so there is no scope to explain away. */}
            {bindingUser === '' ? <Text size="sm" c="dimmed">{t('bindingsHint')}</Text>
                : bindings.isLoading ? <SkeletonRows count={2} />
                  : bindings.isError ? <QueryError retry={() => void bindings.refetch()} />
                    : !bindings.data?.length ? <Text size="sm" c="dimmed">{t('bindingsEmpty')}</Text>
                      : (
                        <TableScrollContainer minWidth={600}>
                          <Table highlightOnHover>
                            <Table.Thead>
                              <Table.Tr>
                                <Table.Th>{t('role')}</Table.Th>
                                <Table.Th>{t('project')}</Table.Th>
                                <Table.Th>{t('createdAt')}</Table.Th>
                                <Table.Th><span className="sr-only">{t('actions')}</span></Table.Th>
                              </Table.Tr>
                            </Table.Thead>
                            <Table.Tbody>
                              {(bindings.data || []).map((row) => (
                                <Table.Tr key={row.id}>
                                  <Table.Td>{roleName(row.role_id)}</Table.Td>
                                  <Table.Td><Code>{row.project_id || '—'}</Code></Table.Td>
                                  <Table.Td>{formatDate(row.created_at)}</Table.Td>
                                  <Table.Td>
                                    <Group justify="flex-end">
                                      <ActionIcon variant="subtle" color="red" aria-label={`${t('revokeBinding')} ${roleName(row.role_id)}`} onClick={() => confirmAction({ title: t('revokeBindingTitle', { role: roleName(row.role_id) }), body: t('revokeBindingBody'), confirmLabel: t('revokeBinding'), onConfirm: () => revoke.mutateAsync({ row, bindings: { projectId: project.id, userId: bindingUser } }) })}>
                                        <Trash2 size={16} />
                                      </ActionIcon>
                                    </Group>
                                  </Table.Td>
                                </Table.Tr>
                              ))}
                            </Table.Tbody>
                          </Table>
                        </TableScrollContainer>
                      )}
          </Stack>
        </Accordion.Panel>
      </Accordion.Item>
    </Accordion>
  )
}

function OidcIdentitiesPanel() {
  const { t } = useTranslation()
  const { project } = useProject()
  const client = useQueryClient()
  const [providerId, setProviderId] = useState('')
  const providerRef = useRef(providerId)
  const projectRef = useRef(project.id)
  providerRef.current = providerId
  projectRef.current = project.id
  const providers = useQuery({ queryKey: ['oidc-providers'], queryFn: () => api<OidcProvider[]>('/api/admin/v1/oidc/providers') })
  useEffect(() => {
    if (!providers.data?.length) { setProviderId(''); return }
    if (!providers.data.some((provider) => provider.id === providerId)) setProviderId(providers.data[0].id)
  }, [providerId, providers.data])
  const identities = useQuery({
    queryKey: ['oidc-identities', providerId],
    queryFn: () => api<OidcIdentity[]>(`/api/admin/v1/oidc/providers/${encodeURIComponent(providerId)}/identities`),
    enabled: Boolean(providerId),
  })
  const save = useMutation({
    mutationFn: ({ provider, body }: { provider: string; body: unknown }) => api(`/api/admin/v1/oidc/providers/${encodeURIComponent(provider)}/identities`, { method: 'POST', body: JSON.stringify(body) }),
    onSuccess: (_data, variables) => { toast.success(t('saved')); void client.invalidateQueries({ queryKey: ['oidc-identities', variables.provider], exact: true }) },
    onError: (error: Error) => toast.error(error.message),
  })
  const remove = useMutation({
    mutationFn: ({ provider, identity, project: capturedProject }: { provider: string; identity: string; project: string }) => {
      if (providerRef.current !== provider || projectRef.current !== capturedProject) throw new Error(t('oidcContextChanged'))
      return api(`/api/admin/v1/oidc/providers/${encodeURIComponent(provider)}/identities/${encodeURIComponent(identity)}`, { method: 'DELETE' })
    },
    onSuccess: (_data, variables) => void client.invalidateQueries({ queryKey: ['oidc-identities', variables.provider], exact: true }),
  })
  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    if (!providerId) return
    const data = new FormData(event.currentTarget)
    try { save.mutate({ provider: providerId, body: { user_id: data.get('user_id'), subject: data.get('subject'), claims: JSON.parse(String(data.get('claims') || '{}')) } }) }
    catch { toast.error(t('invalidJson')) }
  }
  return (
    <>
      <PageHeader title={t('identities')} description={t('identitiesDescription')} />
      {providers.isLoading ? <SkeletonRows /> : providers.isError ? <QueryError retry={() => void providers.refetch()} /> : !providers.data?.length ? (
        <EmptyState icon={<KeyRound />} title={t('identities')} copy={t('oidcProvidersEmpty')} />
      ) : (
        <Stack gap="lg">
          <Select label={t('oidcProvider')} value={providerId} onChange={(value) => value && setProviderId(value)} data={providers.data.map((provider) => ({ value: provider.id, label: provider.name }))} allowDeselect={false} />
          {identities.isLoading ? <SkeletonRows /> : identities.isError ? <QueryError retry={() => void identities.refetch()} /> : !identities.data?.length ? (
            <EmptyState icon={<KeyRound />} title={t('identities')} copy={t('identityEmpty')} />
          ) : (
            <Stack gap="sm" role="list" aria-label={t('identities')}>
              {identities.data.map((identity) => (
                <Paper key={identity.id} withBorder p="md" role="listitem">
                  <SimpleGrid cols={{ base: 1, sm: 3 }} spacing="md" verticalSpacing="sm">
                    <Stack gap={2}><Text size="sm" c="dimmed">{t('subject')}</Text><Code style={{ overflowWrap: 'anywhere' }}>{identity.subject}</Code></Stack>
                    <Stack gap={2}><Text size="sm" c="dimmed">{t('boundUser')}</Text><Code style={{ overflowWrap: 'anywhere' }}>{identity.user_id}</Code></Stack>
                    <Group justify="flex-end" align="flex-end">
                      <Button variant="subtle" color="red" leftSection={<Link2Off size={16} />} aria-label={`${t('unlinkIdentity')} ${identity.subject}`} onClick={() => confirmAction({
                        title: t('unlinkIdentityTitle', { subject: identity.subject }),
                        body: t('unlinkIdentityBody', { user: identity.user_id }),
                        confirmLabel: t('unlinkIdentity'),
                        onConfirm: () => remove.mutateAsync({ provider: identity.provider_id, identity: identity.id, project: project.id }),
                      })}>{t('unlinkIdentity')}</Button>
                    </Group>
                  </SimpleGrid>
                </Paper>
              ))}
            </Stack>
          )}
        </Stack>
      )}
      {providers.data?.length ? <Paper withBorder p="lg" mt="lg">
        <Title order={2} mb="md">{t('linkIdentity')}</Title>
        <form onSubmit={submit}>
          <Stack gap="md">
            <TextInput name="user_id" label={t('userId')} required />
            <TextInput name="subject" label={t('subject')} required />
            <Textarea name="claims" label={t('claims')} defaultValue="{}" />
            <Button type="submit" loading={save.isPending}>{t('linkIdentity')}</Button>
          </Stack>
        </form>
      </Paper> : null}
    </>
  )
}

type OidcProvider = { id: string; name: string }
type OidcIdentity = { id: string; provider_id: string; user_id: string; subject: string; created_at: number }

type InvitationRow = { id: string; email: string; role_id: string; expires_at: number; accepted_at: number | null; max_uses: number; use_count: number }
function InvitationsPanel() {
  const { t } = useTranslation()
  const { project } = useProject()
  const client = useQueryClient()
  const [open, setOpen] = useState(false)
  const [delivery, setDelivery] = useState('')
  const [role, setRole] = useState('')
  const path = `/api/admin/v1/projects/${project.id}/invitations`
  const query = useQuery({ queryKey: ['invitations', project.id], queryFn: () => api<InvitationRow[]>(path) })
  const roles = useQuery({ queryKey: ['roles', project.id], queryFn: () => api<Array<{ id: string; name: string }>>(`/api/admin/v1/projects/${project.id}/roles`) })
  const roleOptions = (roles.data || []).map((item) => ({ value: item.id, label: item.name }))
  const create = useMutation({ mutationFn: (body: unknown) => api<{ invitation: InvitationRow; token: string }>(path, { method: 'POST', body: JSON.stringify(body) }), onSuccess: (data) => { void client.invalidateQueries({ queryKey: ['invitations', project.id] }); setOpen(false); setDelivery(`${location.origin}/invite?token=${encodeURIComponent(data.token)}`) }, onError: (error: Error) => toast.error(error.message) })
  const remove = useMutation({ mutationFn: (id: string) => api(`${path}/${id}`, { method: 'DELETE' }), onSuccess: () => void client.invalidateQueries({ queryKey: ['invitations', project.id] }) })
  const submit = (event: FormEvent<HTMLFormElement>) => { event.preventDefault(); const data = new FormData(event.currentTarget); create.mutate({ email: data.get('email'), role_id: role, expires_in_seconds: Number(data.get('expires_in_seconds')), max_uses: Number(data.get('max_uses')) }) }
  // A project with no invitations showed a bare table header: the list now says
  // what it is for and carries the one action that fills it.
  const invite = () => { setRole(roleOptions[0]?.value || ''); setOpen(true) }
  return (
    <>
      <PageHeader title={t('invitations')} description={t('invitationsDescription')} action={query.data?.length ? <Button leftSection={<Plus size={17} />} onClick={invite}>{t('inviteUser')}</Button> : undefined} />
      {query.isLoading ? <SkeletonRows /> : query.isError ? <QueryError retry={() => void query.refetch()} /> : !query.data?.length ? <EmptyState icon={<Mail />} title={t('invitations')} copy={t('invitationEmpty')} action={<Button onClick={invite}>{t('inviteUser')}</Button>} /> : (
        <TableScrollContainer minWidth={600}>
          <Table stickyHeader highlightOnHover>
            <Table.Thead>
              <Table.Tr>
                <Table.Th>{t('email')}</Table.Th>
                <Table.Th>{t('role')}</Table.Th>
                <Table.Th>{t('expiresAt')}</Table.Th>
                <Table.Th>{t('uses')}</Table.Th>
                <Table.Th>{t('status')}</Table.Th>
                <Table.Th />
              </Table.Tr>
            </Table.Thead>
            <Table.Tbody>
              {(query.data || []).map((row) => (
                <Table.Tr key={row.id}>
                  <Table.Td><strong>{row.email}</strong></Table.Td>
                  <Table.Td>{roleOptions.find((option) => option.value === row.role_id)?.label || row.role_id}</Table.Td>
                  <Table.Td>{formatDate(row.expires_at)}</Table.Td>
                  <Table.Td>{t('usesProgress', { used: row.use_count, maximum: row.max_uses })}</Table.Td>
                  <Table.Td>{row.expires_at <= Math.floor(Date.now() / 1000) ? t('invitationExpired') : row.use_count >= row.max_uses ? t('accepted') : t('pending')}</Table.Td>
                  <Table.Td>
                    <ActionIcon variant="subtle" color="red" aria-label={`${t('delete')} ${row.email}`} onClick={() => confirmAction({ title: t('deleteInvitationTitle', { email: row.email }), body: t('deleteInvitationBody'), onConfirm: () => remove.mutateAsync(row.id) })}>
                      <Trash2 size={16} />
                    </ActionIcon>
                  </Table.Td>
                </Table.Tr>
              ))}
            </Table.Tbody>
          </Table>
        </TableScrollContainer>
      )}
      <Modal open={open} onOpenChange={setOpen} title={t('inviteUser')}>
        <form onSubmit={submit}>
          <Stack gap="md">
            <TextInput name="email" type="email" label={t('email')} required />
            {/* The invite is bound to a project role: if that list cannot be read
                the dialog says so and retries, rather than offering no role. */}
            {roles.isError
              ? <InlineQueryError message={t('projectRolesUnavailable')} onRetry={() => void roles.refetch()} />
              : <SelectField label={t('role')} value={role} onValueChange={setRole} options={roleOptions} />}
            <NumberInput name="expires_in_seconds" label={t('expiresSeconds')} min={60} max={2592000} defaultValue={604800} required hideControls />
            <NumberInput name="max_uses" label={t('maxUses')} min={1} max={100} defaultValue={1} required />
            <Button type="submit" disabled={!role || roles.isError}>{t('inviteUser')}</Button>
          </Stack>
        </form>
      </Modal>
      <Modal open={Boolean(delivery)} onOpenChange={(value) => !value && setDelivery('')} title={t('invitationCreated')} description={t('invitationOneTime')}>
        <Stack gap="md">
          <Code block>{delivery}</Code>
          <Button onClick={() => void navigator.clipboard.writeText(delivery)}>{t('copy')}</Button>
        </Stack>
      </Modal>
    </>
  )
}

type MembershipRow = Document & { id: string; project_id: string; user_id: string; email: string | null; display_name: string | null; role_id: string; status: string; created_at: number; updated_at: number }

/**
 * Memberships are per-project, like roles and invitations, so they are a tab on
 * the Access page that follows the project selector instead of a second picker.
 * The columns are the fields `access::MembershipView` returns: the gateway joins
 * the user, so the identity column reads the nullable `display_name`/`email`
 * while the role name is resolved against the project's own role list and the
 * ownership badge against the selected project's `owner_user_id`. Listing only
 * needs `project:read`; adding and removing need `project:manage`, which is what
 * `access::upsert_membership` and `access::remove_membership` authorize.
 */
function MembersPanel({ canWrite }: { canWrite: boolean }) {
  const { t } = useTranslation()
  const { project } = useProject()
  const client = useQueryClient()
  const [mode, setMode] = useState<'add' | 'edit' | null>(null)
  const [editing, setEditing] = useState<MembershipRow | null>(null)
  const [formProjectId, setFormProjectId] = useState('')
  const [userId, setUserId] = useState('')
  const [userIdError, setUserIdError] = useState<string | null>(null)
  const [formError, setFormError] = useState<string | null>(null)
  const [role, setRole] = useState('')
  const [status, setStatus] = useState('active')
  // Same query key as the roles panel and the invitations panel, so the project
  // role list is fetched once and shared.
  const roles = useQuery({ queryKey: ['roles', project.id], queryFn: () => api<Array<{ id: string; name: string }>>(`/api/admin/v1/projects/${project.id}/roles`) })
  const roleOptions = (roles.data || []).map((item) => ({ value: item.id, label: item.name }))
  const roleName = (roleId: string) => roleOptions.find((option) => option.value === roleId)?.label
  type MembershipWrite = { projectId: string; user_id: string; role_id: string; status: string }
  const save = useMutation({
    mutationFn: ({ projectId, ...body }: MembershipWrite) => api<MembershipRow>(`/api/admin/v1/projects/${projectId}/members`, { method: 'POST', body: JSON.stringify(body) }),
    // A project switch cannot redirect the refresh to the project now on screen:
    // the variables name the roster this POST actually changed.
    onSuccess: (_member, variables) => { toast.success(t('memberAdded')); close(); void client.invalidateQueries({ queryKey: ['resource', variables.projectId, 'members'] }) },
    onError: (error: Error) => setFormError(error.message),
  })
  // No error toast: the shared confirmation dialog owns the failure, so a refused
  // removal is reported where the decision was made rather than twice.
  const remove = useMutation({
    mutationFn: ({ projectId, userId }: { projectId: string; userId: string }) => api(`/api/admin/v1/projects/${projectId}/members/${userId}`, { method: 'DELETE' }),
    onSuccess: (_result, variables) => { toast.success(t('memberRemoved')); void client.invalidateQueries({ queryKey: ['resource', variables.projectId, 'members'] }) },
  })
  const close = () => { setMode(null); setEditing(null); setFormProjectId(''); setUserId(''); setUserIdError(null); setFormError(null); setRole(''); setStatus('active') }
  const beginAdd = () => { setMode('add'); setEditing(null); setFormProjectId(project.id); setUserId(''); setUserIdError(null); setFormError(null); setRole(''); setStatus('active') }
  const beginEdit = (row: MembershipRow) => { setMode('edit'); setEditing(row); setFormProjectId(project.id); setUserId(row.user_id); setUserIdError(null); setFormError(null); setRole(row.role_id); setStatus(row.status) }
  const formProjectChanged = Boolean(formProjectId) && formProjectId !== project.id
  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    setFormError(null)
    // The dialog may outlive a permission-loading interval because AccessPage keeps
    // its panel state. Never reinterpret a form opened for project A as a write to B.
    if (!canWrite || formProjectChanged) { setFormError(t('memberProjectChanged')); return }
    // The route takes a user id, not a name or an email, so an empty one is
    // reported in the form instead of being sent as a membership for nobody.
    if (!userId.trim()) { setUserIdError(t('memberUserIdRequired')); return }
    save.mutate({ projectId: formProjectId, user_id: userId.trim(), role_id: role, status })
  }
  const memberLabel = (row: MembershipRow) => row.display_name || row.email || row.user_id
  const identity = (row: MembershipRow) => <Stack gap={0}><Text size="sm">{row.display_name || row.email || <Code>{row.user_id}</Code>}</Text>{row.display_name && row.email && <Text size="sm" c="dimmed">{row.email}</Text>}</Stack>
  const owner = (row: MembershipRow) => Boolean(project.owner_user_id) && project.owner_user_id === row.user_id
  const memberStatus = (row: MembershipRow) => row.status === 'active' ? <Badge variant="light" color="teal">{t('memberStatusActive')}</Badge> : row.status === 'suspended' ? <Badge variant="light" color="gray">{t('memberStatusSuspended')}</Badge> : displayValue(row.status)
  const rowActions = (document: Document) => {
    const row = document as MembershipRow
    // The backend fixes the owner membership to the owner role and active state;
    // omitting both edit and remove avoids controls that can only be refused.
    if (!canWrite || owner(row)) return null
    return <>
      <Button variant="subtle" size="compact-sm" aria-label={t('editMemberFor', { user: memberLabel(row) })} onClick={() => beginEdit(row)}>{t('edit')}</Button>
      <ActionIcon
        variant="subtle"
        color="red"
        aria-label={t('removeMemberFromProject', { user: memberLabel(row) })}
        onClick={() => {
          const projectId = project.id
          confirmAction({ title: t('removeMemberTitle', { user: memberLabel(row), project: project.name }), body: t('removeMemberBody'), confirmLabel: t('removeMember'), onConfirm: () => remove.mutateAsync({ projectId, userId: row.user_id }) })
        }}
      >
        <UserMinus size={16} />
      </ActionIcon>
    </>
  }
  return (
    <>
      <ResourcePage
        resource="members"
        endpoint={(projectId) => `/api/admin/v1/projects/${projectId}/members`}
        title={t('members')}
        description={t('membersDescription')}
        empty={t('memberEmpty')}
        immutable
        headerAction={canWrite ? <Button leftSection={<Plus size={17} />} onClick={beginAdd}>{t('addMember')}</Button> : undefined}
        emptyAction={canWrite ? <Button onClick={beginAdd}>{t('addMember')}</Button> : undefined}
        columns={[
          { key: 'member_identity', label: t('memberIdentity'), render: (_value, row) => identity(row as MembershipRow) },
          { key: 'role_id', label: t('role'), render: (value) => roleName(String(value)) || displayValue(value) },
          { key: 'ownership', label: t('memberOwnership'), render: (_value, row) => <Badge variant="light" color={owner(row as MembershipRow) ? 'pangolin' : 'gray'}>{owner(row as MembershipRow) ? t('memberOwner') : t('memberMember')}</Badge> },
          { key: 'status', label: t('status'), render: (_value, row) => memberStatus(row as MembershipRow) },
          { key: 'created_at', label: t('memberCreated'), render: (value) => formatDate(value) },
          { key: 'updated_at', label: t('memberUpdated'), render: (value) => formatDate(value) },
        ]}
        rowActions={rowActions}
        mobilePrimary={(row) => identity(row as MembershipRow)}
        mobilePrimaryKey="member_identity"
        mobileHiddenKeys={['status']}
        mobileStatus={(row) => memberStatus(row as MembershipRow)}
      />
      <Modal open={mode !== null} onOpenChange={(value) => { if (!value) close() }} title={mode === 'edit' && editing ? t('editMemberFor', { user: memberLabel(editing) }) : t('addMember')} description={t('membersDescription')}>
        <form onSubmit={submit}>
          <Stack gap="md">
            <TextInput name="user_id" label={t('userId')} description={mode === 'add' ? t('memberUserIdHint') : undefined} value={userId} onChange={(event) => { setUserId(event.target.value); if (userIdError) setUserIdError(null); if (formError) setFormError(null) }} error={userIdError} disabled={mode === 'edit'} autoFocus={mode === 'add'} />
            {roles.isLoading
              ? <Stack gap={4}><Text size="sm" fw={500}>{t('role')}</Text><SkeletonRows count={1} /></Stack>
              : roles.isError
                ? <Stack gap={4}><Text size="sm" fw={500}>{t('role')}</Text><InlineQueryError message={t('memberRolesUnavailable')} onRetry={() => void roles.refetch()} /></Stack>
                : <SelectField label={t('role')} value={role} onValueChange={(value) => { setRole(value); if (formError) setFormError(null) }} options={formProjectChanged ? [] : roleOptions} />}
            <SelectField label={t('status')} value={status} onValueChange={(value) => { setStatus(value); if (formError) setFormError(null) }} options={[{ value: 'active', label: t('memberStatusActive') }, { value: 'suspended', label: t('memberStatusSuspended') }]} />
            {formProjectChanged && <Text role="alert" size="sm" c="red">{t('memberProjectChanged')}</Text>}
            {formError && <Text role="alert" size="sm" c="red">{formError}</Text>}
            <Group justify="flex-end" gap="xs" pt="xs">
              <Button variant="default" type="button" onClick={close}>{t('cancel')}</Button>
              <Button type="submit" loading={save.isPending} disabled={!role || roles.isError || formProjectChanged || !canWrite}>{mode === 'edit' ? t('saveMemberChanges') : t('addMember')}</Button>
            </Group>
          </Stack>
        </form>
      </Modal>
    </>
  )
}
