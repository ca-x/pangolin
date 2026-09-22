import { Alert, Anchor, Button, Code, FileInput, Group, Loader, NumberInput, Paper, Select, SimpleGrid, Stack, Switch, Text, Textarea, Title } from '@mantine/core'
import { useQuery } from '@tanstack/react-query'
import { Link } from 'react-router'
import { AlertTriangle, Send, Square } from 'lucide-react'
import { useEffect, useRef, useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { api } from '../api'
import { InlineQueryError } from '../components'
import { UNMEASURED, formatCount, formatMicros } from '../observability'
import { projectOperationPath, useProject } from '../project'
import { PageHeader } from './shared'

/**
 * How the request ended. `interrupted` is the one the protocol did not name: a
 * stream that ended without any terminal event, which is not evidence of
 * completion. The three protocol reasons are the gateway's own vocabulary
 * (`src/orchestration/stream.rs::response_failure`).
 */
type FailureReason = 'failed' | 'incomplete' | 'cancelled' | 'interrupted'
type Outcome =
  | { kind: 'idle' }
  | { kind: 'streaming' }
  | { kind: 'done' }
  | { kind: 'stopped' }
  /** `code` is the HTTP status of a refused call; 0 when the response itself reported the failure. */
  | { kind: 'failed'; code: number; reason: FailureReason | null; type: string; message: string; raw: string }

/** A terminal event of the Responses protocol, or of the gateway's error envelope. */
type Terminal = { kind: 'completed' } | { kind: 'failed'; reason: FailureReason; type: string; message: string | null }

const PROTOCOL_REASONS: Record<string, FailureReason> = {
  'response.failed': 'failed',
  'response.incomplete': 'incomplete',
  'response.cancelled': 'cancelled',
  'response.canceled': 'cancelled',
}

const REASON_KEYS: Record<FailureReason, string> = { failed: 'statusFailed', incomplete: 'statusIncomplete', cancelled: 'statusCancelled', interrupted: 'statusInterrupted' }
/** The copy used when a failure carries no public message of its own. */
const REASON_COPY: Record<FailureReason, string> = { failed: 'playgroundFailed', incomplete: 'playgroundIncomplete', cancelled: 'playgroundCancelled', interrupted: 'playgroundInterrupted' }

/** The selected key's server-evaluated view of `/v1/responses`. No token enters
 * the browser: the session names a project-owned key by internal id. */
function useReachableModels(keyId: string, projectId: string) {
  const [models, setModels] = useState<string[] | null>(null)
  const [loading, setLoading] = useState(false)
  const [failed, setFailed] = useState(false)
  const [attempt, setAttempt] = useState(0)
  useEffect(() => {
    if (!keyId) { setModels(null); setLoading(false); setFailed(false); return }
    const controller = new AbortController()
    setModels(null)
    setLoading(true)
    setFailed(false)
    const query = new URLSearchParams({ api_key_id: keyId })
    fetch(`/api/admin/v1/projects/${encodeURIComponent(projectId)}/playground/models?${query}`, { credentials: 'same-origin', signal: controller.signal })
      .then((response) => (response.ok ? response.json() : null))
      .then((body) => {
        if (controller.signal.aborted) return
        if (body === null) { setModels(null); setFailed(true); return }
        const values = (body?.data ?? [])
          .map((entry: unknown) => typeof entry === 'string' ? entry : (entry as { id?: unknown })?.id)
          .filter((id: unknown): id is string => typeof id === 'string' && id.length > 0)
        setModels(values)
      })
      .catch(() => { if (!controller.signal.aborted) { setModels(null); setFailed(true) } })
      .finally(() => { if (!controller.signal.aborted) setLoading(false) })
    return () => controller.abort()
  }, [keyId, projectId, attempt])
  // A failed lookup is not "this key reaches nothing": the picker must not claim
  // that, so the failure travels with its own retry.
  return { models, loading, failed, retry: () => setAttempt((count) => count + 1) }
}

/** The gateway's error contract is `{error:{type,message}}`; a pass-through body may not be JSON at all. */
function readFailure(status: number, body: string): Outcome {
  try {
    const parsed = JSON.parse(body)
    const error = parsed?.error ?? {}
    return { kind: 'failed', code: status, reason: null, type: String(error.type ?? 'error'), message: String(error.message ?? body.slice(0, 300)), raw: body }
  } catch {
    return { kind: 'failed', code: status, reason: null, type: 'error', message: body.slice(0, 300), raw: body }
  }
}

/**
 * Only the public half of an error envelope: the protocol's `type` and `message`.
 * The gateway can pass an upstream envelope through verbatim (`ErrorMode::PassThrough`),
 * and that envelope may carry private upstream material, so nothing else is read
 * and the raw event is never rendered.
 */
function publicError(value: unknown): { type: string; message: string | null } {
  const error = value as { type?: unknown; message?: unknown } | null | undefined
  return {
    type: typeof error?.type === 'string' ? error.type : '',
    message: typeof error?.message === 'string' && error.message ? error.message : null,
  }
}

/**
 * Classifies one Responses SSE event. This mirrors the gateway's own terminal
 * classification (`src/orchestration/stream.rs::response_failure`) so the console
 * and the record agree about what happened: `response.completed` is a success only
 * when `response.status` is `completed` and no error is present, and every other
 * terminal is a failure with its own reason.
 */
function readTerminal(value: unknown): Terminal | null {
  const event = value as { type?: unknown; error?: unknown; response?: { status?: unknown; error?: unknown } } | null
  const type = typeof event?.type === 'string' ? event.type : ''
  const status = typeof event?.response?.status === 'string' ? event.response.status : ''
  const envelope = event?.response?.error
  const reason = PROTOCOL_REASONS[type] ?? (type === 'error' ? 'failed' : null)
  if (reason) {
    const error = publicError(type === 'error' ? event?.error : envelope)
    return { kind: 'failed', reason, type: error.type, message: error.message }
  }
  if (type !== 'response.completed') return null
  if (status === 'completed' && !envelope) return { kind: 'completed' }
  const error = publicError(envelope)
  return { kind: 'failed', reason: status === 'incomplete' ? 'incomplete' : status === 'cancelled' || status === 'canceled' ? 'cancelled' : 'failed', type: error.type, message: error.message }
}

/**
 * The same classification for a body that arrived as one JSON document. A 200 with
 * a complete document is the whole response, so only a document that reports a
 * failure of its own is read as one: an absent `status` is not a claim of failure
 * and must not become one.
 */
function readDocument(value: unknown): Terminal | null {
  const document = value as { status?: unknown; error?: unknown } | null
  const status = typeof document?.status === 'string' ? document.status : ''
  const error = publicError(document?.error)
  if (status === 'completed' && !document?.error) return { kind: 'completed' }
  const reason = status === 'incomplete' ? 'incomplete' : status === 'cancelled' || status === 'canceled' ? 'cancelled' : status === 'failed' || document?.error ? 'failed' : null
  return reason ? { kind: 'failed', reason, type: error.type, message: error.message } : null
}

/**
 * The text of a `/v1/responses` body that arrived as one JSON document. The
 * Responses shape carries `output[].content[].text`; `output_text` is the
 * convenience field some gateways add. Anything else is shown as it arrived,
 * because the panel's job is the raw response.
 */
function readAnswer(body: unknown): string {
  const payload = body as { output_text?: unknown; output?: Array<{ content?: Array<{ text?: unknown }> }> }
  if (typeof payload?.output_text === 'string') return payload.output_text
  const parts = (payload?.output ?? []).flatMap((item) => item?.content ?? []).map((part) => (typeof part?.text === 'string' ? part.text : '')).filter(Boolean)
  return parts.length ? parts.join('') : JSON.stringify(body, null, 2)
}

/** A terminal event, as the panel's outcome. A failure keeps only its public half. */
function settle(terminal: Terminal): Outcome {
  return terminal.kind === 'completed'
    ? { kind: 'done' }
    : { kind: 'failed', code: 0, reason: terminal.reason, type: terminal.type, message: terminal.message ?? '', raw: '' }
}

/**
 * The record-system view of one request, as the operations resource returns it.
 * `id` is the record's own UUID — the identity the request-detail page opens — and it
 * is only known once this read has answered.
 */
type RequestRecord = {
  id: string
  /** The record also carries `executions` and `cost_items`; this compact view reads only the usage. */
  usage?: Array<{ input_tokens?: number | null; output_tokens?: number | null; total_cost_micros?: number | null }>
}

type PlaygroundKey = { id: string; name: string; enabled: boolean; expires_at: number | null; budget_micros: number | null; spent_micros: number | null }
type ChatTurn = { id: number; role: 'user' | 'assistant'; content: string }
type PlaygroundChannel = { id: string; name: string; enabled: boolean }

/**
 * The usage rows of a request, summed. A row that carries no number contributes
 * nothing, and a total that nothing was measured for is `null` — which the console
 * prints as `—` rather than inventing a zero.
 */
function sumUsage(rows: RequestRecord['usage'], key: 'input_tokens' | 'output_tokens' | 'total_cost_micros'): number | null {
  let total = 0
  let measured = false
  for (const row of rows ?? []) {
    const value = row?.[key]
    if (typeof value === 'number' && Number.isFinite(value) && value >= 0) { total += value; measured = true }
  }
  return measured ? total : null
}

export default function PlaygroundPage() {
  const { t } = useTranslation()
  const { project } = useProject()
  const [keyId, setKeyId] = useState('')
  const [providerId, setProviderId] = useState('__gateway__')
  const [selection, setSelection] = useState({ scope: '', model: '' })
  const [stream, setStream] = useState(true)
  const [outcome, setOutcome] = useState<Outcome>({ kind: 'idle' })
  const [text, setText] = useState('')
  const [history, setHistory] = useState<ChatTurn[]>([])
  const [draft, setDraft] = useState('')
  const [attachments, setAttachments] = useState<File[]>([])
  const [attachmentError, setAttachmentError] = useState('')
  const [reasoning, setReasoning] = useState('')
  const [requestId, setRequestId] = useState<string | null>(null)
  /** The run the recorded facts belong to: a new run must not show the previous run's. */
  const [runId, setRunId] = useState(0)
  const controller = useRef<AbortController | null>(null)
  const formRef = useRef<HTMLFormElement | null>(null)
  /** The run that owns the panel. A superseded or unmounted run must not write to it. */
  const run = useRef(0)
  const stopping = useRef(false)
  const keys = useQuery({
    queryKey: ['playground-keys', project.id],
    queryFn: () => api<PlaygroundKey[]>(`/api/admin/v1/projects/${encodeURIComponent(project.id)}/api-keys`),
  })
  const channels = useQuery({
    queryKey: ['playground-channels', project.id],
    queryFn: () => api<{ data: PlaygroundChannel[] }>(`${projectOperationPath(project.id, 'channels')}?limit=500`),
  })
  const now = Math.floor(Date.now() / 1000)
  const keyRows = Array.isArray(keys.data) ? keys.data : []
  const usableKeys = keyRows.filter((key) => key.enabled
    && (key.expires_at == null || key.expires_at > now)
    && (key.budget_micros == null || key.spent_micros == null || key.spent_micros < key.budget_micros))
  const selectedKey = usableKeys.some((key) => key.id === keyId) ? keyId : ''
  const { models, loading: modelsLoading, failed: modelsFailed, retry: retryModels } = useReachableModels(selectedKey, project.id)
  // A selection belongs to the exact project/key policy view that produced it.
  // It also has to remain in the latest server answer, including after retry.
  const modelScope = `${project.id}\u0000${selectedKey}`
  const model = selection.scope === modelScope && models?.includes(selection.model) ? selection.model : ''
  const busy = outcome.kind === 'streaming'
  const finished = outcome.kind === 'done' || outcome.kind === 'stopped' || outcome.kind === 'failed'

  useEffect(() => () => { run.current += 1; controller.current?.abort() }, [])
  useEffect(() => {
    run.current += 1
    controller.current?.abort()
    setKeyId('')
    setSelection({ scope: '', model: '' })
    setProviderId('__gateway__')
    setHistory([])
    setDraft('')
    setAttachments([])
    setAttachmentError('')
    setReasoning('')
    setText('')
    setOutcome({ kind: 'idle' })
  }, [project.id])
  useEffect(() => {
    if (!selectedKey && usableKeys.length > 0) setKeyId(usableKeys[0].id)
  }, [selectedKey, usableKeys])

  // The response header names the request; the record that holds its tokens and
  // cost is the authoritative one. It is read once the request has reached a
  // terminal outcome — while the stream is still running the record has nothing
  // settled in it yet — and the key carries the project, the request id and the
  // run. The header id is the caller's and a caller can repeat it, so the run is
  // part of the key: a new run reads its own record instead of showing the one
  // that belonged to the run before it.
  const record = useQuery({
    queryKey: ['playground-request-record', project.id, runId, requestId],
    queryFn: () => api<RequestRecord>(projectOperationPath(project.id, 'requests', String(requestId))),
    enabled: Boolean(requestId) && finished,
    retry: false,
  })
  // The link opens the record by its own id, and only once that read has returned it.
  // The header id is the compatibility lookup, not a navigation target: two requests
  // can share it, so a link built from it can open the wrong row. While the read is in
  // flight, or after it failed, there is no internal id to link with — and none is
  // invented from the header.
  const internalRequestId = typeof record.data?.id === 'string' && record.data.id ? record.data.id : null
  const usage = record.data?.usage
  const count = (value: number | null) => value === null ? UNMEASURED : formatCount(value)
  const money = (value: number | null) => value === null ? UNMEASURED : formatMicros(value)
  const facts: Array<[string, string]> = [
    [t('inputTokens'), count(sumUsage(usage, 'input_tokens'))],
    [t('outputTokens'), count(sumUsage(usage, 'output_tokens'))],
    [t('cost'), money(sumUsage(usage, 'total_cost_micros'))],
  ]

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    const data = new FormData(event.currentTarget)
    const userText = String(data.get('input') ?? '').trim()
    if (!userText || !selectedKey || !model) return
    const nextHistory: ChatTurn[] = [...history, { id: Date.now(), role: 'user', content: userText }]
    setHistory(nextHistory)
    setDraft('')
    setReasoning('')
    const id = ++run.current
    stopping.current = false
    controller.current?.abort()
    const abort = new AbortController()
    controller.current = abort
    setOutcome({ kind: 'streaming' })
    setText('')
    setRequestId(null)
    setRunId(id)
    try {
      const media = await Promise.all(attachments.map((file) => new Promise<string>((resolve, reject) => {
        const reader = new FileReader()
        reader.onload = () => resolve(String(reader.result))
        reader.onerror = () => reject(new Error('attachment could not be read'))
        reader.readAsDataURL(file)
      })))
      setAttachments([])
      const input = [
        ...history.map((turn) => ({ role: turn.role, content: turn.content })),
        {
          role: 'user',
          content: media.length
            ? [{ type: 'input_text', text: userText }, ...media.map((image_url) => ({ type: 'input_image', image_url }))]
            : userText,
        },
      ]
      const response = await fetch(`/api/admin/v1/projects/${encodeURIComponent(project.id)}/playground/chat`, {
        method: 'POST',
        signal: abort.signal,
        credentials: 'same-origin',
        headers: { 'Content-Type': 'application/json', 'X-Pangolin-CSRF': '1' },
        body: JSON.stringify({
          api_key_id: selectedKey,
          ...(providerId !== '__gateway__' ? { provider_id: providerId } : {}),
          payload: {
            model,
            input,
            stream,
            ...(data.get('instructions') ? { instructions: data.get('instructions') } : {}),
            ...(data.get('temperature') ? { temperature: Number(data.get('temperature')) } : {}),
            ...(data.get('max_output_tokens') ? { max_output_tokens: Number(data.get('max_output_tokens')) } : {}),
          },
        }),
      })
      if (id !== run.current) return
      setRequestId(response.headers.get('x-request-id'))
      if (!response.ok || !response.body) {
        const body = await response.text()
        // A superseded run must not write to the panel it no longer owns.
        if (id !== run.current) return
        setOutcome(readFailure(response.status, body))
        return
      }
      // An upstream that cannot stream answers one JSON document. Reading it as
      // SSE would show nothing at all, so the body is read for what it is — and a
      // body that is not JSON at all is shown as it arrived.
      if (!stream) {
        const raw = await response.text()
        if (id !== run.current) return
        let parsed: unknown = null
        try { parsed = JSON.parse(raw) } catch {
          const answer = raw.slice(0, 4000)
          setText(answer)
          setHistory((current) => [...current, { id: Date.now(), role: 'assistant', content: answer }])
          setOutcome({ kind: 'done' })
          return
        }
        // The document is classified *before* anything of it is rendered. A 200
        // that reports its own failure shows the public error fields and nothing
        // else — never its `output`, and never the document itself, which can
        // carry a nested cause the console must not surface.
        const terminal = readDocument(parsed)
        if (terminal?.kind === 'failed') { setOutcome(settle(terminal)); return }
        // Otherwise the 200 is the whole response, and a document that reports no
        // failure of its own is a completed call.
        const answer = readAnswer(parsed)
        setText(answer)
        setHistory((current) => [...current, { id: Date.now(), role: 'assistant', content: answer }])
        setOutcome({ kind: 'done' })
        return
      }
      // The server commits streamed bytes and never retries them, so a read error
      // here is terminal: report it, never re-issue the request.
      const reader = response.body.getReader()
      const decoder = new TextDecoder()
      let buffer = ''
      let terminal: Terminal | null = null
      let answer = ''
      for (;;) {
        const { done, value } = await reader.read()
        // Bytes that arrive after another submission took the panel belong to the
        // run that asked for them, not to the panel.
        if (id !== run.current) return
        if (done) break
        buffer += decoder.decode(value, { stream: true })
        const lines = buffer.split('\n')
        buffer = lines.pop() ?? ''
        for (const line of lines) {
          if (!line.startsWith('data:')) continue
          const payload = line.slice(5).trim()
          if (!payload || payload === '[DONE]') continue
          try {
            const event = JSON.parse(payload)
            // The first terminal decides. A stream that keeps talking after it —
            // another terminal, or more text — cannot change the outcome, and
            // text after the terminal is not part of the response. The terminal is
            // applied as soon as it is read: it is the protocol's own end of the
            // response, so the panel must not wait for a connection to close.
            if (terminal) continue
            if (event.type === 'response.output_text.delta' && typeof event.delta === 'string') {
              answer += event.delta
              setText(answer)
            }
            if (['response.reasoning_summary_text.delta', 'response.reasoning_text.delta'].includes(event.type) && typeof event.delta === 'string') {
              setReasoning((current) => current + event.delta)
            }
            const next = readTerminal(event)
            if (next) { terminal = next; setOutcome(settle(next)) }
          } catch { /* a partial frame is not an error; the next read completes it */ }
        }
      }
      if (id !== run.current) return
      // The protocol owes a terminal event: a stream that ends without one is not
      // known to be complete, and EOF is not evidence that it is. A terminal that
      // did arrive has already decided the panel and is never overwritten here.
      if (!terminal) setOutcome(settle({ kind: 'failed', reason: 'interrupted', type: '', message: null }))
      else if (terminal.kind === 'completed' && answer) setHistory((current) => [...current, { id: Date.now(), role: 'assistant', content: answer }])
    } catch (error) {
      if (id !== run.current) return
      if ((error as Error)?.name === 'AbortError') {
        // An operator stop is an outcome of its own, and the text that already
        // streamed stays where it is.
        setOutcome(stopping.current ? { kind: 'stopped' } : { kind: 'failed', code: 0, reason: 'cancelled', type: '', message: '', raw: '' })
        return
      }
      setOutcome({ kind: 'failed', code: 0, reason: null, type: 'network', message: error instanceof Error ? error.message : String(error), raw: '' })
    }
  }

  const clearConversation = () => {
    run.current += 1
    controller.current?.abort()
    setHistory([])
    setDraft('')
    setAttachments([])
    setAttachmentError('')
    setReasoning('')
    setText('')
    setRequestId(null)
    setOutcome({ kind: 'idle' })
  }

  const regenerate = () => {
    let lastUser = -1
    for (let index = history.length - 1; index >= 0; index -= 1) {
      if (history[index].role === 'user') { lastUser = index; break }
    }
    if (lastUser < 0 || busy) return
    const prompt = history[lastUser].content
    setHistory(history.slice(0, lastUser))
    setDraft(prompt)
    window.setTimeout(() => formRef.current?.requestSubmit(), 0)
  }

  const label = outcome.kind === 'done' ? t('playgroundCompleted') : outcome.kind === 'stopped' ? t('playgroundStopped') : outcome.kind === 'failed' ? t(REASON_KEYS[outcome.reason ?? 'failed']) : ''
  const failureMessage = outcome.kind === 'failed' ? outcome.message || t(REASON_COPY[outcome.reason ?? 'failed']) : ''
  const failureTitle = outcome.kind === 'failed' ? (outcome.reason ? [t(REASON_KEYS[outcome.reason]), outcome.type].filter(Boolean).join(' · ') : `${outcome.code || ''} ${outcome.type}`.trim()) : ''
  const assistantSettled = outcome.kind === 'done' && history[history.length - 1]?.role === 'assistant'

  return (
    <>
      <PageHeader title={t('playground')} description={t('playgroundDescription')} />
      <SimpleGrid cols={{ base: 1, md: 2 }} spacing="md">
        <Paper withBorder p="lg">
          <form ref={formRef} onSubmit={submit}>
            <Stack gap="md">
              {keys.isError ? <InlineQueryError message={t('optionsUnavailable')} onRetry={() => void keys.refetch()} /> : <Select
                label={t('apiKey')}
                aria-label={t('apiKey')}
                required
                searchable
                value={selectedKey || null}
                onChange={(value) => { setKeyId(value ?? ''); setSelection({ scope: '', model: '' }) }}
                data={usableKeys.map((key) => ({ value: key.id, label: key.name }))}
                disabled={keys.isLoading || usableKeys.length === 0}
                rightSection={keys.isLoading ? <Loader size={16} /> : undefined}
                nothingFoundMessage={t('noUsablePlaygroundKeys')}
                description={!keys.isLoading && usableKeys.length === 0 ? t('noUsablePlaygroundKeys') : t('playgroundSessionKeyHint')}
              />}
              {channels.isError ? <InlineQueryError message={t('optionsUnavailable')} onRetry={() => void channels.refetch()} /> : <Select
                label={t('playgroundSource')}
                value={providerId}
                onChange={(value) => setProviderId(value ?? '__gateway__')}
                data={[
                  { value: '__gateway__', label: t('playgroundModelGateway') },
                  ...(channels.data?.data ?? []).filter((channel) => channel.enabled).map((channel) => ({ value: channel.id, label: channel.name })),
                ]}
                disabled={channels.isLoading}
                rightSection={channels.isLoading ? <Loader size={16} /> : undefined}
              />}
              {/* AxonHub's own model picker is a strict selector — "no free-form
                  values are allowed" — because a typed model that the key cannot
                  route fails later as a routing error. Searchable so it still
                  feels like a text field, but the value must come from the list. */}
              {modelsFailed ? (
                <InlineQueryError message={t('optionsUnavailable')} onRetry={retryModels} />
              ) : (
                <Select
                  name="model"
                  label={t('model')}
                  required
                  searchable
                  value={model || null}
                  onChange={(value) => setSelection({ scope: modelScope, model: value ?? '' })}
                  data={models ?? []}
                  disabled={!selectedKey || !models || modelsLoading || models.length === 0}
                  rightSection={modelsLoading ? <Loader size={16} /> : undefined}
                  nothingFoundMessage={t('noReachableModels')}
                />
              )}
              {!modelsFailed && !modelsLoading && models?.length === 0 && (
                <Alert role="status" color="gray" title={t('noReachableModels')}>
                  <Button type="button" variant="subtle" size="xs" onClick={retryModels}>{t('retry')}</Button>
                </Alert>
              )}
              {/* The gateway needs the upstream to stream; an upstream that answers a
                  single JSON document fails a streaming call, so the operator can
                  turn streaming off and read the document instead. */}
              <Switch
                name="stream"
                checked={stream}
                onChange={(event) => setStream(event.currentTarget.checked)}
                label={t('streamResponse')}
                description={t('streamResponseHint')}
              />
              <Group grow align="flex-start">
                <NumberInput name="temperature" label={t('temperature')} min={0} max={2} step={0.1} decimalScale={2} hideControls />
                <NumberInput name="max_output_tokens" label={t('maxOutputTokens')} min={1} max={1000000} hideControls />
              </Group>
              <Textarea name="instructions" label={t('instructions')} rows={2} />
              <Textarea name="input" label={t('input')} rows={8} required value={draft} onChange={(event) => setDraft(event.currentTarget.value)} />
              <FileInput
                label={t('attachments')}
                description={t('playgroundAttachmentHint')}
                accept="image/*"
                multiple
                value={attachments}
                error={attachmentError || undefined}
                onChange={(files) => {
                  const next = files ?? []
                  if (next.length > 4 || next.some((file) => !file.type.startsWith('image/') || file.size > 5 * 1024 * 1024)) {
                    setAttachmentError(t('playgroundAttachmentInvalid'))
                    setAttachments([])
                  } else {
                    setAttachmentError('')
                    setAttachments(next)
                  }
                }}
              />
              <Group gap="sm">
                <Button type="submit" leftSection={<Send size={17} />} loading={busy} disabled={busy || modelsFailed || modelsLoading || !model}>
                  {t('sendRequest')}
                </Button>
                {busy && (
                  <Button variant="default" leftSection={<Square size={15} />} onClick={() => { stopping.current = true; controller.current?.abort() }}>
                    {t('stop')}
                  </Button>
                )}
                {!busy && history.some((turn) => turn.role === 'user') && <Button type="button" variant="default" onClick={regenerate}>{t('regenerate')}</Button>}
                <Button type="button" variant="subtle" color="gray" onClick={clearConversation} disabled={busy && !history.length}>{t('clearConversation')}</Button>
              </Group>
            </Stack>
          </form>
        </Paper>
        <Paper withBorder p="lg" aria-live="polite">
          <Stack gap="md">
            <Group justify="space-between" align="center">
              <Title order={2}>{t('response')}</Title>
              <Group gap="sm" align="center" style={{ minWidth: 0 }}>
                {/* The caller's own correlation id stays on the panel; the link below is
                    the record's identity, which is the only unambiguous one. The id is
                    bounded — a client may send up to 128 characters — so it truncates
                    instead of pushing the panel wide at 375px. */}
                {requestId && <Text size="xs" c="dimmed" className="mono-cell" aria-label={t('requestExternalId')} truncate>{requestId}</Text>}
                {internalRequestId && <Anchor component={Link} to={`/operations/requests/${internalRequestId}`} size="sm">{t('viewRequest')}</Anchor>}
              </Group>
            </Group>
            {history.length > 0 && <Stack gap="xs" role="log" aria-label={t('playgroundConversation')}>
              {history.map((turn) => <Paper key={turn.id} withBorder p="sm" bg={turn.role === 'user' ? 'var(--mantine-color-default-hover)' : undefined}>
                <Text size="xs" c="dimmed" fw={600}>{turn.role === 'user' ? t('you') : t('assistant')}</Text>
                <Text size="sm" style={{ whiteSpace: 'pre-wrap', overflowWrap: 'anywhere' }}>{turn.content}</Text>
              </Paper>)}
            </Stack>}
            {reasoning && <Paper withBorder p="sm" role="region" aria-label={t('reasoning')}>
              <Text size="xs" c="dimmed" fw={600}>{t('reasoning')}</Text>
              <Text size="sm" style={{ whiteSpace: 'pre-wrap', overflowWrap: 'anywhere' }}>{reasoning}</Text>
            </Paper>}
            {outcome.kind === 'failed' && (
              <Alert variant="light" color="red" icon={<AlertTriangle size={17} />} title={failureTitle}>
                <Text size="sm">{failureMessage}</Text>
                {outcome.raw && outcome.raw !== failureMessage && (
                  <Code block mt="xs" style={{ maxHeight: 160, overflow: 'auto' }}>{outcome.raw.slice(0, 4000)}</Code>
                )}
              </Alert>
            )}
            {outcome.kind === 'stopped' && (
              <Alert variant="light" color="gray" icon={<Square size={17} />} title={t('playgroundStopped')}>
                <Text size="sm">{t('playgroundStoppedCopy')}</Text>
              </Alert>
            )}
            {/* The panel is in one state at a time. A failure or a stop is reported by
                its own alert — which already carries the copy — so the idle "send a
                request" text must not sit under it as a second, contradictory state.
                Streamed text is never discarded: it is what the request produced. */}
            {assistantSettled || (outcome.kind === 'failed' || outcome.kind === 'stopped') && !text ? null : <Code block style={{ minHeight: 420, whiteSpace: 'pre-wrap' }}>
              {text || (busy ? t('streaming') : outcome.kind === 'done' ? t('playgroundNoOutput') : t('playgroundEmpty'))}
            </Code>}
            {/* The terminal outcome and the recorded facts of the same request, in
                one place: the response panel must not read as a success while the
                record says otherwise, or the other way round. */}
            {finished && (
              <Group gap="lg" wrap="wrap" align="baseline">
                <Text size="xs" c="dimmed">{`${t('outcome')}: ${label}`}</Text>
                {record.isError ? (
                  <InlineQueryError message={t('playgroundUsageUnavailable')} onRetry={() => void record.refetch()} />
                ) : record.isLoading ? (
                  <Text size="xs" c="dimmed">{t('playgroundUsageLoading')}</Text>
                ) : (
                  facts.map(([name, value]) => <Text size="xs" c="dimmed" key={name}>{`${name}: ${value}`}</Text>)
                )}
              </Group>
            )}
          </Stack>
        </Paper>
      </SimpleGrid>
    </>
  )
}
