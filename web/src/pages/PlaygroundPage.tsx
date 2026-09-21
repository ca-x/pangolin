import { Button, Code, Paper, PasswordInput, SimpleGrid, Stack, Textarea, TextInput, Title } from '@mantine/core'
import { Send } from 'lucide-react'
import { useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { PageHeader } from './shared'

export default function PlaygroundPage() {
  const { t } = useTranslation()
  const [busy, setBusy] = useState(false)
  const [result, setResult] = useState('')
  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setBusy(true);
    setResult('');
    const data = new FormData(event.currentTarget);
    try {
      const response = await fetch('/v1/responses', { method: 'POST', headers: { 'Content-Type': 'application/json', 'Authorization': `Bearer ${data.get('token')}` }, body: JSON.stringify({ model: data.get('model'), input: data.get('input') }) });
      const text = await response.text();
      setResult(JSON.stringify(JSON.parse(text), null, 2))
    } catch (error) { setResult(error instanceof Error ? error.message : String(error)) } finally { setBusy(false) }
  }
  return (
    <>
      <PageHeader title={t('playground')} description={t('playgroundDescription')} />
      <SimpleGrid cols={{ base: 1, md: 2 }} spacing="md">
        <Paper withBorder p="lg">
          <form onSubmit={submit}>
            <Stack gap="md">
              <PasswordInput name="token" label={t('apiKey')} description={t('playgroundKeyHint')} autoComplete="off" required />
              <TextInput name="model" label={t('model')} required />
              <Textarea name="input" label={t('input')} rows={10} required />
              <Button type="submit" leftSection={<Send size={17} />} loading={busy}>
                {busy ? t('loading') : t('sendRequest')}
              </Button>
            </Stack>
          </form>
        </Paper>
        <Paper withBorder p="lg" aria-live="polite">
          <Stack gap="md">
            <Title order={2}>{t('response')}</Title>
            <Code block style={{ minHeight: 420, whiteSpace: 'pre-wrap' }}>
              {result || t('playgroundEmpty')}
            </Code>
          </Stack>
        </Paper>
      </SimpleGrid>
    </>
  )
}