import { Badge, Button, Card, Code, Group, Pagination, Stack, Tabs, Text, Title } from '@mantine/core'
import { CheckCircle2, Copy, Download, Terminal } from 'lucide-react'
import { useMemo, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Modal } from '../components'

export type PayloadViewerProps = {
  requestId: string
  endpoint: string
  stream: boolean | null
  captured: boolean
  requestRaw: string | null
  responseRaw: string | null
}
export type PayloadChunk = {
  index: number
  event: string
  dataText: string
  value?: unknown
  terminal: boolean
}

type StreamEnvelope = { version: number; event: string; data: unknown; terminal: boolean }
type ConversationMessage = { index: number; role: string; content: string; raw: unknown }
type Conversation = { messages: ConversationMessage[] }

const isEnvelope = (value: unknown): value is StreamEnvelope => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false
  const envelope = value as Record<string, unknown>
  return envelope.version === 1 && typeof envelope.event === 'string' && typeof envelope.terminal === 'boolean' && 'data' in envelope
}

const record = (value: unknown): Record<string, unknown> | null => value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : null

function protocolTerminal(event: string, dataText: string, value: unknown): boolean {
  if (dataText.trim() === '[DONE]' || event === 'error') return true
  const type = typeof record(value)?.type === 'string' ? String(record(value)?.type) : ''
  if (['message_stop', 'response.completed', 'response.failed', 'response.incomplete', 'response.cancelled', 'response.canceled'].includes(event)) return true
  if (['message_stop', 'response.completed', 'response.failed', 'response.incomplete', 'response.cancelled', 'response.canceled', 'error'].includes(type)) return true
  const candidates = record(value)?.candidates
  return Array.isArray(candidates) && candidates.length > 0 && candidates.every((candidate) => typeof record(candidate)?.finishReason === 'string')
}

function parseSse(raw: string): PayloadChunk[] {
  const chunks: PayloadChunk[] = []
  for (const frame of raw.replaceAll('\r\n', '\n').split(/\n\n+/)) {
    let event = 'message'
    const data: string[] = []
    for (const line of frame.split('\n')) {
      if (line.startsWith('event:')) event = line.slice(6).trimStart()
      else if (line.startsWith('data:')) data.push(line.slice(5).trimStart())
    }
    if (!data.length) continue
    const dataText = data.join('\n')
    let value: unknown
    try { value = JSON.parse(dataText) } catch { value = dataText }
    chunks.push({ index: chunks.length, event, dataText, value, terminal: protocolTerminal(event, dataText, value) })
  }
  return chunks
}

function contentText(content: unknown): string {
  if (typeof content === 'string') return content
  if (content == null) return ''
  if (Array.isArray(content)) return content.map((part) => {
    if (typeof part === 'string') return part
    const item = record(part)
    if (typeof item?.text === 'string') return item.text
    if (typeof item?.content === 'string') return item.content
    return JSON.stringify(part, null, 2)
  }).filter(Boolean).join('\n')
  return JSON.stringify(content, null, 2)
}

export function parseConversation(raw: string | null, endpoint: string, response = false): Conversation | null {
  if (!raw || !endpoint) return null
  let body: unknown
  try { body = JSON.parse(raw) } catch { return null }
  const document = record(body)
  if (!document) return null
  const messages: ConversationMessage[] = []
  if (!response && endpoint === '/v1/messages' && document.system != null) {
    const content = contentText(document.system)
    if (content) messages.push({ index: 0, role: 'system', content, raw: document.system })
  }
  let source: unknown[] | null = null
  if (!response && Array.isArray(document.messages)) source = document.messages
  else if (response && Array.isArray(document.choices)) source = document.choices.map((choice) => record(choice)?.message ?? choice)
  else if (!response && Array.isArray(document.input)) source = document.input
  else if (!response && endpoint === '/v1/responses' && typeof document.input === 'string') source = [{ role: 'user', content: document.input }]
  else if (response && Array.isArray(document.output)) source = document.output
  else if (!response && Array.isArray(document.contents)) source = document.contents.map((content) => {
    const item = record(content)
    return item ? { ...item, content: item.parts } : content
  })
  else if (response && Array.isArray(document.candidates)) source = document.candidates.map((candidate) => record(candidate)?.content ?? candidate)
  if (!source) return null
  for (const rawMessage of source) {
    const message = record(rawMessage)
    if (!message) continue
    const content = contentText(message.content ?? message.parts)
    const role = typeof message.role === 'string' ? message.role : 'unknown'
    if (content || role !== 'unknown') messages.push({ index: messages.length, role, content: content || JSON.stringify(rawMessage, null, 2), raw: rawMessage })
  }
  return messages.length ? { messages } : null
}

function prettyRaw(raw: string | null): string {
  if (!raw) return '—'
  try { return JSON.stringify(JSON.parse(raw), null, 2) } catch { return raw }
}

function downloadRaw(raw: string, name: string) {
  const url = URL.createObjectURL(new Blob([raw], { type: 'application/json' }))
  const anchor = document.createElement('a')
  anchor.href = url
  anchor.download = name
  anchor.click()
  URL.revokeObjectURL(url)
}

const sensitiveName = (name: string) => {
  const lower = name.toLowerCase().replaceAll('-', '_')
  return lower.includes('authorization') || lower.includes('cookie') || lower.includes('secret') || lower.includes('password') || lower.includes('credential') || lower.includes('api_key') || lower === 'apikey' || lower === 'token' || lower.endsWith('_token')
}

const mediaName = (name: string) => {
  const lower = name.toLowerCase().replaceAll('-', '_')
  return lower.includes('base64') || ['b64_json', 'file_data', 'audio_data'].includes(lower)
}

function safeCurlValue(value: unknown, key = ''): unknown {
  if (sensitiveName(key)) return '[REDACTED]'
  if (mediaName(key)) return '[MEDIA OMITTED]'
  if (typeof value === 'string' && value.trimStart().startsWith('data:')) return '[MEDIA OMITTED]'
  if (Array.isArray(value)) return value.map((item) => safeCurlValue(item))
  const object = record(value)
  if (object) return Object.fromEntries(Object.entries(object).map(([name, item]) => [name, safeCurlValue(item, name)]))
  return value
}

const shellQuote = (value: string) => `'${value.replaceAll("'", `'"'"'`)}'`

export function generateCurl(endpoint: string, raw: string | null): string | null {
  if (!raw || !endpoint.startsWith('/') || endpoint.startsWith('//') || /[\r\n]/.test(endpoint)) return null
  let body: unknown
  try { body = JSON.parse(raw) } catch { return null }
  const url = `${window.location.origin}${endpoint}`
  return [
    `curl ${shellQuote(url)}`,
    `  -H ${shellQuote('Content-Type: application/json')}`,
    '  -H "Authorization: Bearer $PANGOLIN_API_KEY"',
    `  --data-raw ${shellQuote(JSON.stringify(safeCurlValue(body)))}`,
  ].join(' \\\n')
}

export function parseStoredChunks(raw: string | null): PayloadChunk[] {
  if (!raw) return []
  if (/^(?:event|data|id|retry):/m.test(raw)) return parseSse(raw)
  const chunks: PayloadChunk[] = []
  for (const line of raw.split(/\r?\n/).filter(Boolean)) {
    try {
      const value: unknown = JSON.parse(line)
      if (isEnvelope(value)) {
        chunks.push({
          index: chunks.length,
          event: value.event,
          dataText: typeof value.data === 'string' ? value.data : JSON.stringify(value.data),
          value: value.data,
          terminal: value.terminal,
        })
      } else {
        const event = value && typeof value === 'object' && !Array.isArray(value) && typeof (value as Record<string, unknown>).type === 'string'
          ? String((value as Record<string, unknown>).type)
          : 'message'
        chunks.push({ index: chunks.length, event, dataText: line, value, terminal: protocolTerminal(event, line, value) })
      }
    } catch {
      return []
    }
  }
  return chunks
}

function ChunksView({ chunks }: { chunks: PayloadChunk[] }) {
  const { t } = useTranslation()
  const [page, setPage] = useState(1)
  const pageSize = 20
  const total = Math.ceil(chunks.length / pageSize)
  const visible = chunks.slice((page - 1) * pageSize, page * pageSize)
  return <Stack gap="sm" role="region" aria-label={t('responseChunks')}>
    <Stack component="ol" gap="sm" p={0} m={0} style={{ listStyle: 'none' }}>
    {visible.map((chunk) => <Card component="li" key={chunk.index} p="sm" withBorder>
      <Group justify="space-between" align="center" mb="xs" wrap="wrap">
        <Group gap="xs">
          <Text size="sm" fw={600}>{t('chunkNumber', { count: chunk.index + 1 })}</Text>
          <Code>{chunk.event}</Code>
        </Group>
        {chunk.terminal && <Badge variant="light" color="teal" leftSection={<CheckCircle2 size={12} aria-hidden />}>{t('terminal')}</Badge>}
      </Group>
      <Code block style={{ maxHeight: 240, overflow: 'auto', whiteSpace: 'pre-wrap', overflowWrap: 'anywhere' }}>{chunk.value === undefined || typeof chunk.value === 'string' ? chunk.dataText : JSON.stringify(chunk.value, null, 2)}</Code>
    </Card>)}
    </Stack>
    {total > 1 && <Group justify="flex-end"><Pagination total={total} value={page} onChange={setPage} getControlProps={(control) => control === 'previous' ? { 'aria-label': t('previousChunksPage') } : control === 'next' ? { 'aria-label': t('nextChunksPage') } : {}} /></Group>}
  </Stack>
}

function ConversationView({ conversation, label }: { conversation: Conversation; label: string }) {
  const { t } = useTranslation()
  return <Stack component="ol" gap="sm" p={0} m={0} style={{ listStyle: 'none' }} role="region" aria-label={label}>
    {conversation.messages.map((message) => <Card component="li" key={message.index} p="sm" withBorder>
      <Text size="sm" fw={600} mb="xs">{t(`payloadRole.${message.role}`, { defaultValue: message.role })}</Text>
      <Text component="pre" size="md" style={{ margin: 0, whiteSpace: 'pre-wrap', overflowWrap: 'anywhere', fontFamily: 'inherit' }}>{message.content}</Text>
    </Card>)}
  </Stack>
}

function BodyPanel({ title, raw, conversation, chunks, response = false, downloadName, downloadLabel, actions }: { title: string; raw: string | null; conversation: Conversation | null; chunks?: PayloadChunk[]; response?: boolean; downloadName: string; downloadLabel: string; actions?: ReactNode }) {
  const { t } = useTranslation()
  const defaultView = chunks?.length ? 'chunks' : conversation ? 'conversation' : 'json'
  return <Card component="section" aria-label={title} p="md">
    <Group justify="space-between" align="center" mb="sm" wrap="wrap">
      <Title order={3}>{title}</Title>
      <Group gap="xs" wrap="wrap">
        {actions}
        {raw && <Button variant="subtle" size="compact-sm" leftSection={<Download size={15} aria-hidden />} aria-label={downloadLabel} onClick={() => downloadRaw(raw, downloadName)}>{t('download')}</Button>}
      </Group>
    </Group>
    <Tabs defaultValue={defaultView} keepMounted={false}>
      <Tabs.List mb="sm">
        {chunks && chunks.length > 0 && <Tabs.Tab value="chunks">{t('chunks')}</Tabs.Tab>}
        {conversation && <Tabs.Tab value="conversation">{t('conversation')}</Tabs.Tab>}
        <Tabs.Tab value="json">{t('json')}</Tabs.Tab>
      </Tabs.List>
      {chunks && chunks.length > 0 && <Tabs.Panel value="chunks"><ChunksView chunks={chunks} /></Tabs.Panel>}
      {conversation && <Tabs.Panel value="conversation"><ConversationView conversation={conversation} label={response ? t('responseConversation') : t('requestConversation')} /></Tabs.Panel>}
      <Tabs.Panel value="json"><Code block role="region" aria-label={t('rawJson')} tabIndex={0} style={{ maxHeight: 360, overflow: 'auto', whiteSpace: 'pre-wrap', overflowWrap: 'anywhere' }}>{prettyRaw(raw)}</Code></Tabs.Panel>
    </Tabs>
  </Card>
}

export function PayloadViewer({ requestId, endpoint, stream, captured, requestRaw, responseRaw }: PayloadViewerProps) {
  const { t } = useTranslation()
  const [curlOpen, setCurlOpen] = useState(false)
  const chunks = useMemo(() => stream ? parseStoredChunks(responseRaw) : [], [responseRaw, stream])
  const requestConversation = useMemo(() => parseConversation(requestRaw, endpoint), [endpoint, requestRaw])
  const responseConversation = useMemo(() => stream ? null : parseConversation(responseRaw, endpoint, true), [endpoint, responseRaw, stream])
  const curl = useMemo(() => generateCurl(endpoint, requestRaw), [endpoint, requestRaw])
  if (!captured) return null
  const safeId = requestId.replace(/[^a-zA-Z0-9._-]+/g, '-').slice(0, 96) || 'request'
  return <Stack gap="lg">
    <BodyPanel title={t('requestPayload')} raw={requestRaw} conversation={requestConversation} downloadName={`pangolin-${safeId}-request.json`} downloadLabel={t('downloadRequestPayload')} actions={curl && <Button variant="default" size="compact-sm" leftSection={<Terminal size={15} aria-hidden />} onClick={() => setCurlOpen(true)}>{t('previewCurl')}</Button>} />
    <BodyPanel title={t('responsePayload')} raw={responseRaw} conversation={responseConversation} chunks={stream ? chunks : undefined} response downloadName={`pangolin-${safeId}-response.json`} downloadLabel={t('downloadResponsePayload')} />
    <Modal open={curlOpen} onOpenChange={setCurlOpen} title={t('curlPreview')} description={t('curlSanitizedHint')}>
      <Stack gap="md">
        <Code block role="region" aria-label={t('curlCommand')} tabIndex={0} style={{ maxHeight: 420, overflow: 'auto', whiteSpace: 'pre-wrap', overflowWrap: 'anywhere' }}>{curl}</Code>
        <Group justify="flex-end"><Button leftSection={<Copy size={15} aria-hidden />} onClick={() => { if (curl) void navigator.clipboard?.writeText(curl) }}>{t('copyCurl')}</Button></Group>
      </Stack>
    </Modal>
  </Stack>
}
