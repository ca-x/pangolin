import { Alert, Anchor, Button, Code, Group, Loader, NumberInput, Paper, PasswordInput, Select, SimpleGrid, Stack, Text, Textarea, Title } from '@mantine/core'
import { Link } from 'react-router'
import { AlertTriangle, Send, Square } from 'lucide-react'
import { useEffect, useRef, useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { PageHeader } from './shared'

type Outcome = { kind: 'idle' } | { kind: 'streaming' } | { kind: 'done' } | { kind: 'failed'; status: number; type: string; message: string; raw: string }

/** The key's own view of what it can reach — the only authoritative list. */
function useReachableModels(token: string) {
  const [models, setModels] = useState<string[] | null>(null)
  const [loading, setLoading] = useState(false)
  useEffect(() => {
    if (token.length < 8) { setModels(null); return }
    const controller = new AbortController()
    setLoading(true)
    fetch('/v1/models', { headers: { Authorization: `Bearer ${token}` }, signal: controller.signal })
      .then((response) => (response.ok ? response.json() : null))
      .then((body) => setModels(body?.data?.map((entry: { id: string }) => entry.id) ?? []))
      .catch(() => setModels(null))
      .finally(() => setLoading(false))
    return () => controller.abort()
  }, [token])
  return { models, loading }
}

/** The gateway's error contract is `{error:{type,message}}`; a pass-through body may not be JSON at all. */
function readFailure(status: number, body: string): Outcome {
  try {
    const parsed = JSON.parse(body)
    const error = parsed?.error ?? {}
    return { kind: 'failed', status, type: String(error.type ?? 'error'), message: String(error.message ?? body.slice(0, 300)), raw: body }
  } catch {
    return { kind: 'failed', status, type: 'error', message: body.slice(0, 300), raw: body }
  }
}

export default function PlaygroundPage() {
  const { t } = useTranslation()
  const [token, setToken] = useState('')
  const [model, setModel] = useState('')
  const [outcome, setOutcome] = useState<Outcome>({ kind: 'idle' })
  const [text, setText] = useState('')
  const [requestId, setRequestId] = useState<string | null>(null)
  const controller = useRef<AbortController | null>(null)
  const { models, loading: modelsLoading } = useReachableModels(token)
  const busy = outcome.kind === 'streaming'

  useEffect(() => () => controller.current?.abort(), [])

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    const data = new FormData(event.currentTarget)
    controller.current?.abort()
    const abort = new AbortController()
    controller.current = abort
    setOutcome({ kind: 'streaming' })
    setText('')
    setRequestId(null)
    try {
      const response = await fetch('/v1/responses', {
        method: 'POST',
        signal: abort.signal,
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${data.get('token')}` },
        body: JSON.stringify({
          model: data.get('model'),
          input: data.get('input'),
          stream: true,
          ...(data.get('instructions') ? { instructions: data.get('instructions') } : {}),
          ...(data.get('temperature') ? { temperature: Number(data.get('temperature')) } : {}),
          ...(data.get('max_output_tokens') ? { max_output_tokens: Number(data.get('max_output_tokens')) } : {}),
        }),
      })
      setRequestId(response.headers.get('x-request-id'))
      if (!response.ok || !response.body) {
        setOutcome(readFailure(response.status, await response.text()))
        return
      }
      // The server commits streamed bytes and never retries them, so a read error
      // here is terminal: report it, never re-issue the request.
      const reader = response.body.getReader()
      const decoder = new TextDecoder()
      let buffer = ''
      for (;;) {
        const { done, value } = await reader.read()
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
            if (event.type === 'response.output_text.delta' && typeof event.delta === 'string') setText((current) => current + event.delta)
          } catch { /* a partial frame is not an error; the next read completes it */ }
        }
      }
      setOutcome({ kind: 'done' })
    } catch (error) {
      if ((error as Error)?.name === 'AbortError') { setOutcome({ kind: 'done' }); return }
      setOutcome({ kind: 'failed', status: 0, type: 'network', message: error instanceof Error ? error.message : String(error), raw: '' })
    }
  }

  return (
    <>
      <PageHeader title={t('playground')} description={t('playgroundDescription')} />
      <SimpleGrid cols={{ base: 1, md: 2 }} spacing="md">
        <Paper withBorder p="lg">
          <form onSubmit={submit}>
            <Stack gap="md">
              <PasswordInput
                name="token"
                label={t('apiKey')}
                description={t('playgroundKeyHint')}
                autoComplete="off"
                required
                value={token}
                onChange={(event) => setToken(event.currentTarget.value)}
              />
              {/* AxonHub's own model picker is a strict selector — "no free-form
                  values are allowed" — because a typed model that the key cannot
                  route fails later as a routing error. Searchable so it still
                  feels like a text field, but the value must come from the list. */}
              <Select
                name="model"
                label={t('model')}
                required
                searchable
                value={model}
                onChange={(value) => setModel(value ?? '')}
                data={models ?? []}
                rightSection={modelsLoading ? <Loader size={16} /> : undefined}
                nothingFoundMessage={t('noReachableModels')}
                description={models && models.length === 0 ? t('noReachableModels') : undefined}
              />
              <Group grow align="flex-start">
                <NumberInput name="temperature" label={t('temperature')} min={0} max={2} step={0.1} decimalScale={2} hideControls />
                <NumberInput name="max_output_tokens" label={t('maxOutputTokens')} min={1} max={1000000} hideControls />
              </Group>
              <Textarea name="instructions" label={t('instructions')} rows={2} />
              <Textarea name="input" label={t('input')} rows={8} required />
              <Group gap="sm">
                <Button type="submit" leftSection={<Send size={17} />} loading={busy} disabled={busy}>
                  {t('sendRequest')}
                </Button>
                {busy && (
                  <Button variant="default" leftSection={<Square size={15} />} onClick={() => controller.current?.abort()}>
                    {t('stop')}
                  </Button>
                )}
              </Group>
            </Stack>
          </form>
        </Paper>
        <Paper withBorder p="lg" aria-live="polite">
          <Stack gap="md">
            <Group justify="space-between" align="center">
              <Title order={2}>{t('response')}</Title>
              {requestId && (
                <Anchor component={Link} to={`/operations/requests/${requestId}`} size="sm">{t('viewRequest')}</Anchor>
              )}
            </Group>
            {outcome.kind === 'failed' && (
              <Alert variant="light" color="red" icon={<AlertTriangle size={17} />} title={`${outcome.status || ''} ${outcome.type}`.trim()}>
                <Text size="sm">{outcome.message}</Text>
                {outcome.raw && outcome.raw !== outcome.message && (
                  <Code block mt="xs" style={{ maxHeight: 160, overflow: 'auto' }}>{outcome.raw.slice(0, 4000)}</Code>
                )}
              </Alert>
            )}
            <Code block style={{ minHeight: 420, whiteSpace: 'pre-wrap' }}>
              {text || (busy ? t('streaming') : t('playgroundEmpty'))}
            </Code>
          </Stack>
        </Paper>
      </SimpleGrid>
    </>
  )
}
