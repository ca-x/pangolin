import { Button, SimpleGrid, Stack, Switch, Text } from '@mantine/core'
import { useMutation, useQuery } from '@tanstack/react-query'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { catalogCard } from '../catalogCard'
import { api, type Document, type Paged } from '../api'
import { InlineQueryError, SelectField } from '../components'
import { projectOperationPath, useProject } from '../project'

type Protocol = { value: string; label: string; capability: string }
const protocols: Protocol[] = [
  { value: '/v1/chat/completions', label: 'Chat Completions', capability: 'chat' },
  { value: '/v1/responses', label: 'Responses', capability: 'responses' },
  { value: '/v1/messages', label: 'Messages', capability: 'messages' },
  { value: '/v1beta/models:generateContent', label: 'Gemini', capability: 'gemini' },
]
const enabled = (row: Document) => row.enabled === true || row.enabled === 1
function supports(kind: string, capabilities: unknown, protocol: Protocol, stream: boolean) {
  const caps = Array.isArray(capabilities) ? capabilities : []
  const endpoint = protocol.capability === 'gemini' && stream ? '/v1beta/models:streamGenerateContent' : protocol.value
  if (!caps.includes(protocol.capability) && !caps.includes(endpoint) && !(caps.includes('chat') && ['messages', 'gemini'].includes(protocol.capability))) return false
  if (kind === 'anthropic') return protocol.capability === 'messages' || (protocol.capability === 'chat' && !stream)
  if (['gemini', 'vertex', 'gcp'].includes(kind)) return protocol.capability === 'gemini' || (protocol.capability === 'chat' && !stream)
  if (kind === 'bedrock') return protocol.capability === 'chat' && !stream
  return !(stream && ['messages', 'gemini'].includes(protocol.capability))
}

/** Scoped query state; remount dependent selections as soon as project/channel changes. */
export function ProbeForm({ providerId, onQueued }: { providerId?: string; onQueued?: () => void }) {
  const { project } = useProject()
  return <ScopedProbeForm key={project.id} providerId={providerId} onQueued={onQueued} />
}
function ScopedProbeForm({ providerId, onQueued }: { providerId?: string; onQueued?: () => void }) {
  const { t } = useTranslation()
  const { project } = useProject()
  const [selected, setSelected] = useState('')
  const channels = useQuery({ queryKey: ['channel-options', project.id], queryFn: () => api<Paged<Document>>(projectOperationPath(project.id, 'channels') + '?limit=500') })
  const available = (channels.data?.data || []).filter(enabled)
  const current = providerId || (available.some((row) => row.id === selected) ? selected : String(available[0]?.id || ''))
  const channel = available.find((row) => row.id === current)
  if (channels.isPending) return <Text size="md" role="status">{t('loading')}</Text>
  if (channels.isError) return <InlineQueryError message={t('optionsUnavailable')} onRetry={() => void channels.refetch()} />
  return <Stack gap="md" aria-label={t('probeConfiguration')}>
    {!providerId && <SelectField label={t('channel')} value={current} onValueChange={setSelected} options={available.map((row) => ({ value: String(row.id), label: String(row.name) }))} />}
    {channel ? <ProbeSelection key={current} channel={channel} onQueued={onQueued} /> : <Text size="md">{t('probeNoChannels')}</Text>}
  </Stack>
}
function ProbeSelection({ channel, onQueued }: { channel: Document; onQueued?: () => void }) {
  const { t } = useTranslation()
  const { project } = useProject()
  const [model, setModel] = useState('')
  const models = useQuery({ queryKey: ['probe-models', project.id], queryFn: () => api<Paged<Document>>(projectOperationPath(project.id, 'models') + '?limit=500') })
  const credentials = useQuery({ queryKey: ['probe-credentials', project.id, channel.id], queryFn: () => api<Paged<Document>>(projectOperationPath(project.id, 'credentials') + `?limit=500&provider_id=${encodeURIComponent(channel.id)}`) })
  const policies = useQuery({ queryKey: ['probe-policies', project.id, channel.id], queryFn: () => api<Paged<Document>>(projectOperationPath(project.id, 'channel-settings') + `?limit=500&provider_id=${encodeURIComponent(channel.id)}`) })
  const eligible = (models.data?.data || []).filter((row) => row.provider_id === channel.id && enabled(row) && row.lifecycle === 'active')
  const selected = eligible.find((row) => row.id === model) || eligible[0]
  const keys = (credentials.data?.data || []).filter((row) => row.provider_id === channel.id && enabled(row) && row.state !== 'unrecoverable')
  const policy = policies.data?.data.find((row) => row.provider_id === channel.id)
  const rules = policy?.model_rules as Record<string, unknown> | undefined
  const metadata = catalogCard(selected?.catalog_metadata)
  const failed = models.isError || credentials.isError || policies.isError
  const loading = models.isPending || credentials.isPending || policies.isPending
  return <Stack gap="md">
    {loading && <Text size="md" role="status">{t('loading')}</Text>}
    {models.isError && <InlineQueryError message={t('probeModelsUnavailable')} onRetry={() => void models.refetch()} />}
    {credentials.isError && <InlineQueryError message={t('probeCredentialsUnavailable')} onRetry={() => void credentials.refetch()} />}
    {policies.isError && <InlineQueryError message={t('optionsUnavailable')} onRetry={() => void policies.refetch()} />}
    {!models.isPending && !models.isError && !eligible.length && <Text size="md">{t('probeNoModels')}</Text>}
    {!credentials.isPending && !credentials.isError && !keys.length && <Text size="md">{t('probeNoCredentials')}</Text>}
    <SelectField label={t('model')} value={String(selected?.id || '')} onValueChange={setModel} options={eligible.map((row) => ({ value: String(row.id), label: String(row.public_name || row.id) }))} />
    <ProbeOptions key={String(selected?.id || '')} channel={channel} model={selected} credentials={keys} blocked={failed || loading} allowStream={rules?.stream !== false && metadata?.capabilities?.streaming !== false} onQueued={onQueued} />
  </Stack>
}
function ProbeOptions({ channel, model, credentials, blocked, allowStream, onQueued }: { channel: Document; model?: Document; credentials: Document[]; blocked: boolean; allowStream: boolean; onQueued?: () => void }) {
  const { t } = useTranslation()
  const { project } = useProject()
  const [credential, setCredential] = useState('')
  const [protocol, setProtocol] = useState('')
  const [stream, setStream] = useState(false)
  const kind = String(channel.kind)
  const available = protocols.filter((item) => supports(kind, model?.capabilities, item, false) || (allowStream && supports(kind, model?.capabilities, item, true)))
  const family = kind === 'anthropic' ? 'messages' : ['gemini', 'vertex', 'gcp'].includes(kind) ? 'gemini' : 'chat'
  const selected = available.find((item) => item.value === protocol) || available.find((item) => item.capability === family) || available[0]
  const canStream = Boolean(selected && allowStream && supports(kind, model?.capabilities, selected, true))
  const canNonstream = Boolean(selected && supports(kind, model?.capabilities, selected, false))
  const streaming = canStream && (stream || !canNonstream)
  const key = credentials.find((row) => row.id === credential) || credentials[0]
  const ready = !blocked && Boolean(model && key && selected && (canNonstream || canStream))
  const run = useMutation({
    mutationFn: () => api(projectOperationPath(project.id, 'probe'), { method: 'POST', body: JSON.stringify({ provider_id: channel.id, model_id: model?.id, credential_id: key?.id, endpoint: selected?.capability === 'gemini' && streaming ? '/v1beta/models:streamGenerateContent' : selected?.value, stream: streaming }) }),
    onSuccess: () => { toast.success(t('jobQueued')); onQueued?.() },
  })
  return <form onSubmit={(event) => { event.preventDefault(); if (ready) run.mutate() }}>
    <Stack gap="md">
      <SimpleGrid cols={{ base: 1, sm: 2 }}>
        <SelectField label={t('probeCredential')} value={String(key?.id || '')} onValueChange={setCredential} options={credentials.map((row) => ({ value: row.id, label: `•••• ${row.suffix || row.id}` }))} />
        <SelectField label={t('probeProtocol')} value={selected?.value || ''} onValueChange={(value) => { setProtocol(value); setStream(false) }} options={available} />
      </SimpleGrid>
      {!blocked && model && !available.length && <Text size="md">{t('probeNoProtocols')}</Text>}
      <Switch label={t('stream')} checked={streaming} disabled={!canStream || !canNonstream} onChange={(event) => setStream(event.currentTarget.checked)} />
      {run.isError && <InlineQueryError message={run.error.message} onRetry={() => { if (ready) run.mutate() }} />}
      <Button type="submit" disabled={!ready} loading={run.isPending} style={{ alignSelf: 'flex-start' }}>{t('runProbe')}</Button>
    </Stack>
  </form>
}
