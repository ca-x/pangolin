import { Accordion, ActionIcon, Button, Code, Group, NumberInput, Paper, PasswordInput, Select, SimpleGrid, Stack, Tabs, Table, TableScrollContainer, Text, TextInput, Textarea, Title } from '@mantine/core'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { KeyRound, Plus, Trash2 } from 'lucide-react'
import { useEffect, useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { api } from '../api'
import { EmptyState, EnabledPill, Modal, SelectField, SkeletonRows } from '../components'
import { useProject } from '../project'
import { PageHeader, QueryError, ResourcePage, displayValue, formatDate } from './shared'

const ACCESS_TABS: Array<[string, string]> = [['keys', 'api_key:manage'], ['profiles', 'api_key:manage'], ['projects', 'project:manage'], ['users', 'user:manage'], ['roles', 'role:manage'], ['invitations', 'project:manage'], ['oidc', 'oidc:manage'], ['identities', 'oidc:manage']]

export default function AccessPage() {
  const { t } = useTranslation()
  const { permissions } = useProject()
  const [tab, setTab] = useState('keys')
  // Mirrors access::authorize: project:manage satisfies project:read, role:manage
  // and api_key:manage. Without this the UI hid tabs the backend would accept.
  const granted = new Set<string>(permissions)
  if (granted.has('project:manage')) for (const implied of ['project:read', 'role:manage', 'api_key:manage']) granted.add(implied)
  const can = (permission: string) => granted.has('*') || granted.has(permission)
  const visible = ACCESS_TABS.filter(([, permission]) => can(permission))
  const active = visible.some(([v]) => v === tab) ? tab : visible[0]?.[0]
  if (!active) return <div><PageHeader title={t('access')} description={t('keysDescription')} /><Paper withBorder p="lg" mt="md"><Text size="sm" c="dimmed">{t('noAccessSection')}</Text></Paper></div>
  return (
    <Tabs keepMounted={false} value={active} onChange={(v) => v && setTab(v)}>
      <Tabs.List mb="lg">
        {visible.map(([value]) => (
          <Tabs.Tab key={value} value={value}>{t(value)}</Tabs.Tab>
        ))}
      </Tabs.List>
      <Tabs.Panel value="keys"><KeysPanel /><KeyLoggingForm /></Tabs.Panel>
      <Tabs.Panel value="profiles">
        <ResourcePage resource="key-profiles" title={t('profiles')} description={t('profilesDescription')} empty={t('profileEmpty')} createLabel={t('addProfile')} columns={[{ key: 'name', label: t('name') }, { key: 'rpm_limit', label: 'RPM' }, { key: 'tpm_limit', label: 'TPM' }, { key: 'budget_micros', label: t('budget') }, { key: 'routing_policy', label: t('routingPolicy') }]} fields={[{ key: 'name', label: t('name'), required: true }, { key: 'rpm_limit', label: 'RPM', kind: 'number' }, { key: 'tpm_limit', label: 'TPM', kind: 'number' }, { key: 'budget_micros', label: t('budgetMicros'), kind: 'number' }, { key: 'routing_policy', label: t('routingPolicy'), kind: 'json', defaultValue: { version: 1 } }, { key: 'mappings', label: t('modelMappings'), kind: 'json', defaultValue: [] }, { key: 'allowed_models', label: t('allowedModels'), kind: 'json', defaultValue: [] }]} />
      </Tabs.Panel>
      <Tabs.Panel value="projects">
        <ResourcePage resource="projects" endpoint="/api/admin/v1/projects" itemEndpoint={(id) => `/api/admin/v1/projects/${id}`} updateMethod="PATCH" title={t('projects')} description={t('projectsDescription')} empty={t('projectEmpty')} createLabel={t('addProject')} columns={[{ key: 'name', label: t('name') }, { key: 'slug', label: t('slug'), mono: true }, { key: 'enabled', label: t('status'), render: (value) => <EnabledPill enabled={value} /> }]} fields={[{ key: 'name', label: t('name'), required: true }, { key: 'slug', label: t('slug'), required: true }, { key: 'owner_user_id', label: t('ownerId') }, { key: 'enabled', label: t('enabled'), kind: 'checkbox' }]} />
      </Tabs.Panel>
      <Tabs.Panel value="users">
        <ResourcePage resource="users" endpoint="/api/admin/v1/users" itemEndpoint={(id) => `/api/admin/v1/users/${id}`} updateMethod="PATCH" title={t('users')} description={t('usersDescription')} empty={t('userEmpty')} createLabel={t('addUser')} columns={[{ key: 'display_name', label: t('name') }, { key: 'email', label: t('email') }, { key: 'role', label: t('role') }, { key: 'enabled', label: t('status'), render: (value) => <EnabledPill enabled={value} /> }]} fields={[{ key: 'email', label: t('email'), required: true }, { key: 'display_name', label: t('name') }, { key: 'password', label: t('password'), kind: 'secret', hint: t('passwordHint') }, { key: 'language', label: t('language'), defaultValue: 'zh-CN' }, { key: 'enabled', label: t('enabled'), kind: 'checkbox' }]} />
      </Tabs.Panel>
      <Tabs.Panel value="roles">
        <ResourcePage resource="roles" canDelete={(row) => !row.is_system} editDisabled={(row) => Boolean(row.is_system)} endpoint={(project) => `/api/admin/v1/projects/${project}/roles`} itemEndpoint={(id, project) => `/api/admin/v1/projects/${project}/roles/${id}`} updateMethod="PATCH" title={t('roles')} description={t('rolesDescription')} empty={t('roleEmpty')} createLabel={t('addRole')} columns={[{ key: 'name', label: t('name') }, { key: 'scope', label: t('scope') }, { key: 'is_system', label: t('systemRole') }]} fields={[{ key: 'name', label: t('name'), required: true }, { key: 'permissions', label: t('permissions'), kind: 'json', defaultValue: ['project:read'] }]} />
        <AssignmentsPanel />
      </Tabs.Panel>
      <Tabs.Panel value="invitations"><InvitationsPanel /></Tabs.Panel>
      <Tabs.Panel value="oidc">
        <ResourcePage resource="oidc" endpoint="/api/admin/v1/oidc/providers" itemEndpoint={(id) => `/api/admin/v1/oidc/providers/${id}`} updateMethod="PATCH" title="OIDC" description={t('oidcDescription')} empty={t('oidcEmpty')} createLabel={t('addOidc')} columns={[{ key: 'name', label: t('name') }, { key: 'issuer_url', label: t('issuer'), mono: true }, { key: 'client_id', label: t('clientId'), mono: true }, { key: 'enabled', label: t('status'), render: (value) => <EnabledPill enabled={value} /> }, { key: 'secret_configured', label: t('secretConfigured') }]} fields={[{ key: 'name', label: t('name'), required: true }, { key: 'issuer_url', label: t('issuer'), required: true }, { key: 'client_id', label: t('clientId'), required: true }, { key: 'client_secret', label: t('clientSecret'), kind: 'secret' }, { key: 'scopes', label: t('scopes'), kind: 'json', defaultValue: ['openid', 'profile', 'email'] }, { key: 'claim_mapping', label: t('claimMapping'), kind: 'json', defaultValue: { version: 1, jit: true, email_claim: 'email', name_claim: 'name', groups_claim: 'groups' } }, { key: 'enabled', label: t('enabled'), kind: 'checkbox' }]} />
      </Tabs.Panel>
      <Tabs.Panel value="identities"><OidcIdentitiesPanel /></Tabs.Panel>
    </Tabs>
  )
}

type ScopedKey = { id: string; name: string; key_prefix: string; budget_micros: number | null; enabled: boolean; key_type: string; profile_id: string | null; expires_at: number | null; allowed_ips_json: string; denied_ips_json: string }

function KeysPanel() {
  const { t } = useTranslation()
  const { project } = useProject()
  const client = useQueryClient()
  const [open, setOpen] = useState(false)
  const [editing, setEditing] = useState<ScopedKey | null>(null)
  const [mode, setMode] = useState('generated')
  const [token, setToken] = useState<string | null>(null)
  const [profile, setProfile] = useState('__none__')
  // Profiles belong to a project, so drop the selection when the project changes.
  useEffect(() => { setProfile('__none__'); setEditProfile('__none__') }, [project.id])
  const [editProfile, setEditProfile] = useState('__none__')
  const path = `/api/admin/v1/projects/${project.id}/api-keys`
  const query = useQuery({ queryKey: ['keys', project.id], queryFn: () => api<ScopedKey[]>(path) })
  const profiles = useQuery({ queryKey: ['profile-options', project.id], queryFn: () => api<{ data: Array<{ id: string; name: string }> }>(`/api/admin/v1/projects/${project.id}/operations/key-profiles?limit=500`) })
  const profileOptions = [{ value: '__none__', label: t('noProfile') }, ...(profiles.data?.data || []).map((profile) => ({ value: profile.id, label: profile.name }))]
  const create = useMutation({ mutationFn: (body: unknown) => api<{ key: ScopedKey; token?: string; mode: string }>(path, { method: 'POST', body: JSON.stringify(body) }), onSuccess: (data) => { void client.invalidateQueries({ queryKey: ['keys', project.id] }); setOpen(false); if (data.token) setToken(data.token); else toast.success(t('importComplete')) }, onError: (error: Error) => toast.error(error.message) })
  const update = useMutation({ mutationFn: ({ id, body }: { id: string; body: unknown }) => api(`${path}/${id}`, { method: 'PATCH', body: JSON.stringify(body) }), onSuccess: () => void client.invalidateQueries({ queryKey: ['keys', project.id] }), onError: (error: Error) => toast.error(error.message) })
  const remove = useMutation({ mutationFn: (id: string) => api(`${path}/${id}`, { method: 'DELETE' }), onSuccess: () => void client.invalidateQueries({ queryKey: ['keys', project.id] }), onError: (error: Error) => toast.error(error.message) })
  const submit = (event: FormEvent<HTMLFormElement>) => { event.preventDefault(); const data = new FormData(event.currentTarget); create.mutate({ name: data.get('name'), token_mode: mode, token: mode === 'import_existing' ? data.get('token') : null, key_type: 'service', scopes: ['gateway:use'], profile_id: data.get('profile_id') === '__none__' ? null : data.get('profile_id') || null, budget_micros: data.get('budget_micros') ? Number(data.get('budget_micros')) : null, expires_at: data.get('expires_at') ? Number(data.get('expires_at')) : null, allowed_ips: String(data.get('allowed_ips') || '').split(',').map((v) => v.trim()).filter(Boolean), denied_ips: String(data.get('denied_ips') || '').split(',').map((v) => v.trim()).filter(Boolean) }) }
  const submitEdit = (event: FormEvent<HTMLFormElement>) => { event.preventDefault(); if (!editing) return; const data = new FormData(event.currentTarget); update.mutate({ id: editing.id, body: { name: data.get('name'), profile_id: data.get('profile_id') === '__none__' ? null : data.get('profile_id') || null, budget_micros: data.get('budget_micros') ? Number(data.get('budget_micros')) : null, expires_at: data.get('expires_at') ? Number(data.get('expires_at')) : null, allowed_ips: String(data.get('allowed_ips') || '').split(',').map((v) => v.trim()).filter(Boolean), denied_ips: String(data.get('denied_ips') || '').split(',').map((v) => v.trim()).filter(Boolean) } }); setEditing(null); setEditProfile('__none__') }
  return (
    <>
      <PageHeader title={t('keys')} description={t('keysDescription')} action={<Button leftSection={<Plus size={17} />} onClick={() => setOpen(true)}>{t('addKey')}</Button>} />
      {query.isError ? <QueryError retry={() => void query.refetch()} /> : query.isLoading ? <SkeletonRows /> : !query.data?.length ? <EmptyState icon={<KeyRound />} title={t('keys')} copy={t('keyEmpty')} action={<Button variant="default" onClick={() => setOpen(true)}>{t('addKey')}</Button>} /> : (
        <TableScrollContainer minWidth={700}>
          <Table stickyHeader highlightOnHover>
            <Table.Thead>
              <Table.Tr>
                <Table.Th>{t('name')}</Table.Th>
                <Table.Th>{t('fingerprint')}</Table.Th>
                <Table.Th>{t('type')}</Table.Th>
                <Table.Th>{t('budget')}</Table.Th>
                <Table.Th>{t('ipPolicy')}</Table.Th>
                <Table.Th>{t('status')}</Table.Th>
                <Table.Th><span className="sr-only">{t('actions')}</span></Table.Th>
              </Table.Tr>
            </Table.Thead>
            <Table.Tbody>
              {query.data.map((key) => (
                <Table.Tr key={key.id}>
                  <Table.Td><strong>{key.name}</strong></Table.Td>
                  <Table.Td><Code>{key.key_prefix}</Code></Table.Td>
                  <Table.Td>{key.key_type}</Table.Td>
                  <Table.Td>{displayValue(key.budget_micros)}</Table.Td>
                  <Table.Td><Code>{key.allowed_ips_json === '[]' && key.denied_ips_json === '[]' ? t('anyIp') : t('restricted')}</Code></Table.Td>
                  <Table.Td>
                    <Button
                      variant="light"
                      color={key.enabled ? 'teal' : 'gray'}
                      size="compact-sm"
                      aria-label={`${key.enabled ? t('disable') : t('enable')} ${key.name}`}
                      onClick={() => update.mutate({ id: key.id, body: { enabled: !key.enabled } })}
                    >
                      {key.enabled ? t('enabled') : t('disabled')}
                    </Button>
                  </Table.Td>
                  <Table.Td>
                    <Group gap={4} justify="flex-end" wrap="nowrap">
                      <Button variant="subtle" size="compact-sm" onClick={() => { setEditing(key); setEditProfile(key.profile_id || '__none__') }}>{t('edit')}</Button>
                      <ActionIcon variant="subtle" color="red" aria-label={`${t('delete')} ${key.name}`} onClick={() => confirm(t('deleteConfirm')) && remove.mutate(key.id)}>
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
      <Modal open={open} onOpenChange={setOpen} title={t('addKey')} description={mode === 'import_existing' ? t('importSecurity') : t('generatedSecurity')}>
        <form onSubmit={submit}>
          <Stack gap="md">
            <SelectField label={t('tokenMode')} value={mode} onValueChange={setMode} options={[{ value: 'generated', label: t('generateToken') }, { value: 'import_existing', label: t('importExisting') }]} />
            {mode === 'import_existing' && <PasswordInput name="token" label={t('existingToken')} description={t('importTokenHint')} minLength={32} maxLength={1024} autoComplete="off" required />}
            <TextInput name="name" label={t('keyName')} required autoFocus />
            <SelectField name="profile_id" label={t('profileId')} value={profile} onValueChange={setProfile} options={profileOptions} />
            <Group gap="md" grow>
              <NumberInput name="budget_micros" label={t('budgetMicros')} min={0} hideControls />
              <NumberInput name="expires_at" label={t('expiresAtEpoch')} min={1} hideControls />
            </Group>
            <Group gap="md" grow>
              <TextInput name="allowed_ips" label={t('allowedIps')} placeholder="192.0.2.0/24" />
              <TextInput name="denied_ips" label={t('deniedIps')} placeholder="192.0.2.10" />
            </Group>
            <Group justify="flex-end" gap="xs" pt="xs">
              <Button variant="default" type="button" onClick={() => setOpen(false)}>{t('cancel')}</Button>
              <Button type="submit" loading={create.isPending}>{t('save')}</Button>
            </Group>
          </Stack>
        </form>
      </Modal>
      <Modal open={Boolean(editing)} onOpenChange={(value) => !value && setEditing(null)} title={t('editKey')}>
        <form onSubmit={submitEdit}>
          <Stack gap="md">
            <TextInput name="name" label={t('keyName')} required defaultValue={editing?.name} />
            <SelectField name="profile_id" label={t('profileId')} value={editProfile} onValueChange={setEditProfile} options={profileOptions} />
            <Group gap="md" grow>
              <NumberInput name="budget_micros" label={t('budgetMicros')} min={0} defaultValue={editing?.budget_micros ?? ''} hideControls />
              <NumberInput name="expires_at" label={t('expiresAtEpoch')} min={1} defaultValue={editing?.expires_at ?? ''} hideControls />
            </Group>
            <Group gap="md" grow>
              <TextInput name="allowed_ips" label={t('allowedIps')} defaultValue={editing ? JSON.parse(editing.allowed_ips_json).join(', ') : ''} />
              <TextInput name="denied_ips" label={t('deniedIps')} defaultValue={editing ? JSON.parse(editing.denied_ips_json).join(', ') : ''} />
            </Group>
            <Button type="submit">{t('save')}</Button>
          </Stack>
        </form>
      </Modal>
      <Modal open={Boolean(token)} onOpenChange={(value) => !value && setToken(null)} title={t('keyCreated')} description={t('keyCreatedHint')}>
        <Stack gap="md">
          <Code block>{token}</Code>
          <Button onClick={() => token && navigator.clipboard.writeText(token)}>{t('copy')}</Button>
        </Stack>
      </Modal>
    </>
  )
}

function KeyLoggingForm() {
  const { t } = useTranslation()
  const { project } = useProject()
  const [level, setLevel] = useState('inherit')
  const save = useMutation({ mutationFn: (body: unknown) => api(`/api/admin/v1/projects/${project.id}/operations/key-logging`, { method: 'POST', body: JSON.stringify(body) }), onSuccess: () => toast.success(t('saved')), onError: (error: Error) => toast.error(error.message) })
  const submit = (event: FormEvent<HTMLFormElement>) => { event.preventDefault(); save.mutate({ api_key_id: new FormData(event.currentTarget).get('api_key_id'), level }) }
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

function AssignmentsPanel() {
  const { t } = useTranslation()
  const { project } = useProject()
  const save = useMutation({ mutationFn: ({ path, body }: { path: string; body: unknown }) => api(path, { method: 'POST', body: JSON.stringify(body) }), onSuccess: () => toast.success(t('saved')), onError: (error: Error) => toast.error(error.message) })
  const member = (event: FormEvent<HTMLFormElement>) => { event.preventDefault(); const data = new FormData(event.currentTarget); save.mutate({ path: `/api/admin/v1/projects/${project.id}/members`, body: { user_id: data.get('user_id'), role_id: data.get('role_id'), status: data.get('status') } }) }
  const binding = (event: FormEvent<HTMLFormElement>) => { event.preventDefault(); const data = new FormData(event.currentTarget); save.mutate({ path: `/api/admin/v1/users/${data.get('user_id')}/role-bindings`, body: { role_id: data.get('role_id'), project_id: project.id } }) }
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
                <Select name="status" label={t('status')} defaultValue="active" data={[{ value: 'active', label: 'active' }, { value: 'suspended', label: 'suspended' }]} />
                <Button type="submit">{t('save')}</Button>
              </Stack>
            </form>
            <form onSubmit={binding}>
              <Stack gap="md">
                <Title order={3}>{t('roleBinding')}</Title>
                <TextInput name="user_id" label={t('userId')} required />
                <TextInput name="role_id" label={t('roleId')} required />
                <Button type="submit">{t('assign')}</Button>
              </Stack>
            </form>
          </SimpleGrid>
        </Accordion.Panel>
      </Accordion.Item>
    </Accordion>
  )
}

function OidcIdentitiesPanel() {
  const { t } = useTranslation()
  const save = useMutation({ mutationFn: ({ provider, body }: { provider: string; body: unknown }) => api(`/api/admin/v1/oidc/providers/${provider}/identities`, { method: 'POST', body: JSON.stringify(body) }), onSuccess: () => toast.success(t('saved')), onError: (error: Error) => toast.error(error.message) })
  const submit = (event: FormEvent<HTMLFormElement>) => { event.preventDefault(); const data = new FormData(event.currentTarget); try { save.mutate({ provider: String(data.get('provider_id')), body: { user_id: data.get('user_id'), subject: data.get('subject'), claims: JSON.parse(String(data.get('claims') || '{}')) } }) } catch { toast.error(t('invalidJson')) } }
  return (
    <>
      <PageHeader title={t('identities')} description={t('identitiesDescription')} />
      <Paper withBorder p="lg">
        <form onSubmit={submit}>
          <Stack gap="md">
            <TextInput name="provider_id" label={t('oidcProviderId')} required />
            <TextInput name="user_id" label={t('userId')} required />
            <TextInput name="subject" label={t('subject')} required />
            <Textarea name="claims" label={t('claims')} defaultValue="{}" />
            <Button type="submit">{t('linkIdentity')}</Button>
          </Stack>
        </form>
      </Paper>
    </>
  )
}

type InvitationRow = { id: string; email: string; role_id: string; expires_at: number; accepted_at: number | null }
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
  const submit = (event: FormEvent<HTMLFormElement>) => { event.preventDefault(); const data = new FormData(event.currentTarget); create.mutate({ email: data.get('email'), role_id: role, expires_in_seconds: Number(data.get('expires_in_seconds')) }) }
  return (
    <>
      <PageHeader title={t('invitations')} description={t('invitationsDescription')} action={<Button leftSection={<Plus size={17} />} onClick={() => { setRole(roleOptions[0]?.value || ''); setOpen(true) }}>{t('inviteUser')}</Button>} />
      {query.isLoading ? <SkeletonRows /> : query.isError ? <QueryError retry={() => void query.refetch()} /> : (
        <TableScrollContainer minWidth={600}>
          <Table stickyHeader highlightOnHover>
            <Table.Thead>
              <Table.Tr>
                <Table.Th>{t('email')}</Table.Th>
                <Table.Th>{t('role')}</Table.Th>
                <Table.Th>{t('expiresAt')}</Table.Th>
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
                  <Table.Td>{row.accepted_at ? t('accepted') : t('pending')}</Table.Td>
                  <Table.Td>
                    <ActionIcon variant="subtle" color="red" aria-label={`${t('delete')} ${row.email}`} onClick={() => remove.mutate(row.id)}>
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
            <SelectField label={t('role')} value={role} onValueChange={setRole} options={roleOptions} />
            <NumberInput name="expires_in_seconds" label={t('expiresSeconds')} min={60} max={2592000} defaultValue={604800} required hideControls />
            <Button type="submit" disabled={!role}>{t('inviteUser')}</Button>
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