import { Button, Code, Group, Stack, Tabs, Text, TextInput } from '@mantine/core'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Copy } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { restoreConnectedOpener } from '../dialogFocus'
import { api } from '../api'
import { InlineQueryError, Modal, SelectField } from '../components'
import { CLIENT_LABELS, SETUP_CLIENTS, clientProtocol, defaultGatewayBase, gatewayUrls, generateClientSetup, type SetupClient } from '../clientSetup'

type ClientModel = { id: string; metadata?: { reasoning_levels?: string[] | null } }
export type ClientSetupDialogProps = { projectId: string; keyId: string; keyName: string; token?: string; title?: string; description?: string; onClose: () => void; opener?: HTMLElement | null; focusScope?: () => string | null }

export function ClientSetupDialog(props: ClientSetupDialogProps) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const opener = useRef(props.opener ?? (document.activeElement instanceof HTMLElement ? document.activeElement : null))
  const currentContext = useRef(`${props.projectId}:${props.keyId}`)
  currentContext.current = `${props.projectId}:${props.keyId}`
  const initialContext = useRef(`${props.projectId}:${props.keyId}`)
  const [closed, setClosed] = useState(false)
  const [token, setToken] = useState(props.token)
  const [client, setClient] = useState<SetupClient>('codex')
  const [baseUrl, setBaseUrl] = useState(defaultGatewayBase)
  const [model, setModel] = useState('')
  const [effort, setEffort] = useState('')
  const [shell, setShell] = useState<'posix' | 'powershell'>('posix')
  const contextChanged = initialContext.current !== `${props.projectId}:${props.keyId}`
  const active = !closed && !contextChanged
  const protocol = clientProtocol(client)
  const queryKey = ['client-models', props.projectId, props.keyId, protocol.endpoint, protocol.stream]
  const query = useQuery({
    queryKey,
    queryFn: () => api<{ models: ClientModel[] }>(`/api/admin/v1/projects/${encodeURIComponent(props.projectId)}/api-keys/${encodeURIComponent(props.keyId)}/client-models?${new URLSearchParams({ endpoint: protocol.endpoint, stream: String(protocol.stream) })}`),
    enabled: active,
    gcTime: 0,
    staleTime: 0,
    retry: false,
  })
  const close = () => {
    setClosed(true); setToken(undefined); setModel(''); setEffort(''); setBaseUrl('')
    queryClient.removeQueries({ queryKey: ['client-models', props.projectId, props.keyId] })
    props.onClose()
    const closedScope = props.focusScope?.()
    restoreConnectedOpener(opener.current, () => currentContext.current === initialContext.current && (!props.focusScope || closedScope != null && props.focusScope() === closedScope))
  }
  useEffect(() => {
    if (contextChanged) { setToken(undefined); setModel(''); setEffort(''); setClosed(true); props.onClose() }
  }, [contextChanged, props.onClose])
  const models = query.data?.models ?? []
  const chosen = models.find((item) => item.id === model) ?? models[0]
  const levels = (chosen?.metadata?.reasoning_levels ?? []).filter((level) => ['minimal', 'low', 'medium', 'high', 'xhigh'].includes(level))
  const supportsEffort = !['claude', 'gemini'].includes(client)
  const effectiveEffort = supportsEffort && levels.includes(effort) ? effort : undefined
  let validBase = true
  try { gatewayUrls(baseUrl) } catch { validBase = false }
  const snippet = active && query.isSuccess && !query.isFetching && chosen && validBase
    ? generateClientSetup({ client, baseUrl, model: chosen.id, token, effort: effectiveEffort, shell }) : ''
  const copy = async (value: string) => {
    try { await navigator.clipboard.writeText(value); toast.success(t('copied')) }
    catch { toast.error(t('copyFailed')) }
  }
  // Unmount sensitive DOM immediately; do not retain it for an overlay exit.
  if (!active) return null
  return <Modal open returnFocus={false} onOpenChange={(open) => { if (!open) close() }} title={props.title ?? t('clientSetupTitle', { name: props.keyName })} description={props.description ?? t('clientSetupHint')}>
    <Stack gap="md" style={{ minWidth: 0 }}>
      {token !== undefined && <><Text size="sm">{t('clientOneTimeHint')}</Text><Code block style={{ overflowWrap: 'anywhere', whiteSpace: 'pre-wrap' }}>{token}</Code><Button variant="default" onClick={() => void copy(token)}>{t('copy')}</Button></>}
      <Tabs value={client} onChange={(value) => { if (value) { setClient(value as SetupClient); setModel(''); setEffort('') } }}>
        <Tabs.List style={{ flexWrap: 'wrap' }}>{SETUP_CLIENTS.map((item) => <Tabs.Tab key={item} value={item}>{CLIENT_LABELS[item]}</Tabs.Tab>)}</Tabs.List>
        <Tabs.Panel value={client} pt="md"><Stack gap="md" style={{ minWidth: 0 }}>
          <TextInput label={t('clientGatewayBase')} description={t('clientGatewayBaseHint')} value={baseUrl} onChange={(event) => setBaseUrl(event.target.value)} error={!validBase ? t('clientBaseInvalid') : undefined} />
          <Text size="sm" c="dimmed" style={{ overflowWrap: 'anywhere' }}>{t('clientProtocol', { endpoint: protocol.endpoint, mode: t(protocol.stream ? 'clientStream' : 'clientNonstream') })}</Text>
          {query.isFetching ? <Text size="sm" role="status">{t('clientModelsLoading')}</Text>
            : query.isError ? <InlineQueryError message={t('clientModelsError')} onRetry={() => void query.refetch()} />
              : !models.length ? <Stack gap="xs"><Text size="sm" role="status">{t('clientModelsEmpty')}</Text><Button variant="default" onClick={() => void query.refetch()}>{t('retry')}</Button></Stack>
                : <>
                  <SelectField label={t('model')} value={chosen?.id ?? ''} onValueChange={(value) => { setModel(value); setEffort('') }} options={models.map((item) => ({ value: item.id, label: item.id }))} />
                  {supportsEffort && levels.length > 0 && <SelectField label={t('clientEffort')} value={effectiveEffort ?? '__default__'} onValueChange={(value) => setEffort(value === '__default__' ? '' : value)} options={[{ value: '__default__', label: t('clientDefaultEffort') }, ...levels.map((value) => ({ value, label: t(`clientEffort_${value}`) }))]} />}
                </>}
          {['claude', 'gemini', 'curl'].includes(client) && <SelectField label={t('clientShell')} value={shell} onValueChange={(value) => setShell(value as 'posix' | 'powershell')} options={[{ value: 'posix', label: 'POSIX' }, { value: 'powershell', label: 'PowerShell' }]} />}
          <Text size="sm" c="dimmed">{t(token === undefined ? 'clientEnvironmentHint' : 'clientSensitiveHint')}</Text>
          {snippet && <><Text size="sm">{t(`clientInstructions_${client}`)}</Text><Code block tabIndex={0} role="region" aria-label={t('clientSnippet')} style={{ maxHeight: 'min(40vh, 360px)', maxWidth: '100%', overflow: 'auto', fontSize: 14 }}>{snippet}</Code><Group justify="flex-end"><Button leftSection={<Copy size={16} />} onClick={() => void copy(snippet)}>{t('clientCopySetup')}</Button></Group></>}
        </Stack></Tabs.Panel>
      </Tabs>
    </Stack>
  </Modal>
}
