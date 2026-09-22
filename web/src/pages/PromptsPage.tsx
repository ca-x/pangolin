import { Alert, Badge, Button, Group, Modal, Paper, Stack, Tabs, Text, Textarea } from '@mantine/core'
import { useMutation } from '@tanstack/react-query'
import { AlertTriangle, CheckCircle2, Circle, Eye } from 'lucide-react'
import { useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { api } from '../api'
import { EnabledPill } from '../components'
import { useProject } from '../project'
import { BulkToggle, ResourcePage } from './shared'

/**
 * A prompt's activation in words. The document is the condition tree
 * `src/orchestration/policy.rs::evaluate` accepts: `{version, all|any: [...]}`
 * over `{field, op, value}` leaves, and a version-only document matches every
 * request. A shape this console does not recognize is reported as such rather
 * than summarized into something it is not.
 */
function conditionFields(value: unknown, fields: string[] = []): string[] {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return fields
  const document = value as Record<string, unknown>
  for (const key of ['all', 'any'] as const) {
    const children = document[key]
    if (Array.isArray(children)) {
      for (const child of children) conditionFields(child, fields)
      return fields
    }
  }
  if (typeof document.field === 'string') fields.push(document.field)
  return fields
}

export function activationSummary(activation: unknown): { mode: 'always' | 'all' | 'any' | 'unknown'; fields: string[] } | null {
  if (activation == null || activation === '') return null
  if (typeof activation !== 'object' || Array.isArray(activation)) return { mode: 'unknown', fields: [] }
  const document = activation as Record<string, unknown>
  const fields = [...new Set(conditionFields(document))]
  for (const key of ['all', 'any'] as const) if (Array.isArray(document[key])) return { mode: key, fields }
  return Object.keys(document).every((key) => key === 'version') ? { mode: 'always', fields: [] } : { mode: 'unknown', fields }
}

function ActivationSummary({ value }: { value: unknown }) {
  const { t } = useTranslation()
  const summary = activationSummary(value)
  if (!summary) return <>—</>
  const label = summary.mode === 'always' ? t('promptActivationAlways')
    : summary.mode === 'all' ? t('promptActivationAll', { count: summary.fields.length })
      : summary.mode === 'any' ? t('promptActivationAny', { count: summary.fields.length })
        : t('promptActivationUnknown')
  return (
    <Stack gap={2}>
      <Text size="sm">{label}</Text>
      {summary.fields.length > 0 && <Text size="sm" c="dimmed" className="mono-cell">{t('promptActivationFields', { fields: summary.fields.join(', ') })}</Text>}
    </Stack>
  )
}

function PromptAction({ value }: { value: unknown }) {
  const { t } = useTranslation()
  if (value === 'prepend') return <>{t('prepend')}</>
  if (value === 'append') return <>{t('append')}</>
  return <>—</>
}

function ProtectionState({ value }: { value: unknown }) {
  const { t } = useTranslation()
  const archived = value === 'archived'
  return <Badge variant="light" color={archived ? 'gray' : 'teal'} leftSection={archived ? <Circle size={9} /> : <CheckCircle2 size={11} />}>{archived ? t('archived') : t('active')}</Badge>
}

type PreviewRule = {
  id: string
  name: string
  description: string
  action: 'deny' | 'redact'
  enabled: boolean
  state: 'active' | 'archived'
  matched: boolean
  result: string
}

function ProtectionPreview() {
  const { t } = useTranslation()
  const { project } = useProject()
  const [open, setOpen] = useState(false)
  const [sample, setSample] = useState('')
  const sampleBytes = new Blob([sample]).size
  const sampleTooLarge = sampleBytes > 16 * 1024
  const preview = useMutation({
    mutationFn: (text: string) => api<{ rules: PreviewRule[] }>(`/api/admin/v1/projects/${project.id}/protection-preview`, { method: 'POST', body: JSON.stringify({ text }) }),
  })
  const run = (event?: FormEvent<HTMLFormElement>) => {
    event?.preventDefault()
    if (sample.length > 0 && !sampleTooLarge) preview.mutate(sample)
  }
  const close = () => {
    setOpen(false)
    preview.reset()
  }
  return <>
    <Group justify="flex-end" mb="md">
      <Button variant="default" leftSection={<Eye size={17} />} onClick={() => setOpen(true)}>{t('previewRules')}</Button>
    </Group>
    <Modal opened={open} onClose={close} title={t('protectionPreviewTitle')} size="lg" classNames={{ content: 'protection-preview-dialog' }} closeButtonProps={{ 'aria-label': t('close') }}>
      <form onSubmit={run}>
        <Stack gap="md">
          <Textarea
            label={t('sampleText')}
            description={t('sampleTextHint')}
            value={sample}
            onChange={(event) => { setSample(event.currentTarget.value); preview.reset() }}
            required
            maxLength={16 * 1024}
            rows={5}
            error={sampleTooLarge ? t('sampleTextTooLarge') : undefined}
          />
          <Group justify="flex-end">
            <Button type="submit" loading={preview.isPending} disabled={sample.length === 0 || sampleTooLarge}>{t('runPreview')}</Button>
          </Group>
          <div aria-live="polite">
            {preview.isPending && <Text role="status" size="sm" c="dimmed">{t('previewLoading')}</Text>}
            {preview.isError && <Alert color="red" variant="light" title={t('previewError')} icon={<AlertTriangle size={18} />}>
              <Stack gap="xs">
                <Text size="sm">{preview.error instanceof Error ? preview.error.message : t('networkError')}</Text>
                <Button variant="outline" color="red" size="compact-sm" onClick={() => preview.mutate(sample)} style={{ alignSelf: 'flex-start' }}>{t('retry')}</Button>
              </Stack>
            </Alert>}
            {preview.isSuccess && preview.data.rules.length === 0 && <Paper withBorder p="md"><Text size="sm" c="dimmed">{t('protectionPreviewEmpty')}</Text></Paper>}
            {preview.isSuccess && preview.data.rules.length > 0 && <Stack gap="sm" role="list" aria-label={t('protectionPreviewResults')}>
              {preview.data.rules.map((rule) => <Paper key={rule.id} withBorder p="md" role="listitem">
                <Stack gap="xs">
                  <Group justify="space-between" align="flex-start" wrap="wrap">
                    <Stack gap={2} style={{ minWidth: 0 }}>
                      <Text fw={600}>{rule.name}</Text>
                      {rule.description && <Text size="sm" c="dimmed">{rule.description}</Text>}
                    </Stack>
                    <Group gap="xs">
                      <ProtectionState value={rule.state} />
                      <EnabledPill enabled={rule.enabled} />
                    </Group>
                  </Group>
                  <Group gap="xs">
                    <Badge variant="light" color={rule.matched ? 'teal' : 'gray'} leftSection={rule.matched ? <CheckCircle2 size={11} /> : <Circle size={9} />}>{rule.matched ? t('matched') : t('noMatch')}</Badge>
                    <Text size="sm" c="dimmed">{rule.action === 'deny' ? t('deny') : t('redact')}</Text>
                  </Group>
                  {rule.matched && <>
                    <Text size="sm" fw={500}>{t('previewResult')}</Text>
                    {rule.action === 'deny' && <Text size="sm" c="dimmed">{t('previewDenyHint')}</Text>}
                    <Text component="pre" className="protection-preview-result">{rule.result}</Text>
                  </>}
                </Stack>
              </Paper>)}
            </Stack>}
          </div>
        </Stack>
      </form>
    </Modal>
  </>
}

export default function PromptsPage() {
  const { t } = useTranslation()
  return (
    <Tabs keepMounted={false} defaultValue="prompts">
      <Tabs.List mb="lg">
        <Tabs.Tab value="prompts">{t('prompts')}</Tabs.Tab>
        <Tabs.Tab value="protection">{t('protection')}</Tabs.Tab>
        <Tabs.Tab value="overrides">{t('overrides')}</Tabs.Tab>
      </Tabs.List>
      <Tabs.Panel value="prompts">
        <ResourcePage resource="prompts" title={t('prompts')} description={t('promptsDescription')} empty={t('promptEmpty')} selectable="prompts" createLabel={t('addPrompt')} columns={[{ key: 'name', label: t('name') }, { key: 'role', label: t('role') }, { key: 'order', label: t('promptOrder') }, { key: 'action', label: t('promptAction'), render: (value) => <PromptAction value={value} /> }, { key: 'content', label: t('content') }, { key: 'activation', label: t('activation'), render: (value) => <ActivationSummary value={value} /> }, { key: 'enabled', label: t('status'), render: (value) => <EnabledPill enabled={value} /> }]} fields={[{ key: 'name', label: t('name'), required: true }, { key: 'role', label: t('role'), required: true, defaultValue: 'system' }, { key: 'order', label: t('promptOrder'), kind: 'number', required: true, defaultValue: 0 }, { key: 'action', label: t('promptAction'), kind: 'select', required: true, defaultValue: 'prepend', options: [{ value: 'prepend', label: t('prepend') }, { value: 'append', label: t('append') }] }, { key: 'content', label: t('content'), kind: 'textarea', required: true }, { key: 'activation', label: t('activation'), kind: 'json', defaultValue: { version: 1 } }, { key: 'enabled', label: t('enabled'), kind: 'checkbox', defaultValue: false }]} />
        {/* Prompts carry the same `enabled` flag as channels and credentials, so
            the same bulk action applies (src/api/operations_api.rs). */}
        <BulkToggle resource="prompts" />
      </Tabs.Panel>
      <Tabs.Panel value="protection">
        <ResourcePage resource="protection" title={t('protection')} description={t('protectionDescription')} empty={t('protectionEmpty')} selectable="protection" createLabel={t('addRule')} notice={<ProtectionPreview />} columns={[{ key: 'name', label: t('name') }, { key: 'description', label: t('description') }, { key: 'content_pattern', label: t('pattern'), mono: true }, { key: 'action', label: t('action') }, { key: 'state', label: t('ruleState'), render: (value) => <ProtectionState value={value} /> }, { key: 'test_mode', label: t('testMode') }, { key: 'enabled', label: t('status'), render: (value) => <EnabledPill enabled={value} /> }]} mobileStatus={(row) => <Group gap="xs"><ProtectionState value={row.state} /><EnabledPill enabled={row.enabled} /></Group>} fields={[{ key: 'name', label: t('name'), required: true }, { key: 'description', label: t('description'), kind: 'textarea' }, { key: 'role_pattern', label: t('rolePattern') }, { key: 'content_pattern', label: t('contentPattern'), required: true }, { key: 'action', label: t('action'), kind: 'select', options: [{ value: 'deny', label: t('deny') }, { value: 'redact', label: t('redact') }] }, { key: 'replacement', label: t('replacement') }, { key: 'scopes', label: t('scopes'), kind: 'json', defaultValue: { version: 1 } }, { key: 'test_mode', label: t('testMode'), kind: 'checkbox', defaultValue: false }, { key: 'enabled', label: t('enabled'), kind: 'checkbox' }, { key: 'state', label: t('ruleState'), kind: 'select', required: true, defaultValue: 'active', options: [{ value: 'active', label: t('active') }, { value: 'archived', label: t('archived') }] }]} />
        <BulkToggle resource="protection" />
      </Tabs.Panel>
      <Tabs.Panel value="overrides">
        <ResourcePage resource="channel-settings" canDelete={false} title={t('overrides')} description={t('overridesDescription')} empty={t('overridesEmpty')} columns={[{ key: 'provider_id', label: t('channelId'), mono: true }, { key: 'parameter_overrides', label: t('parameterOverrides') }, { key: 'endpoint_mappings', label: t('endpointMappings') }, { key: 'retry_statuses', label: t('retryStatuses') }]} fields={[{ key: 'provider_id', label: t('channelId'), required: true }, { key: 'endpoint_mappings', label: t('endpointMappings'), kind: 'json', defaultValue: { version: 1 } }, { key: 'model_rules', label: t('modelRules'), kind: 'json', defaultValue: { version: 1 } }, { key: 'parameter_overrides', label: t('parameterOverrides'), kind: 'json', defaultValue: { version: 1 } }, { key: 'retry_statuses', label: t('retryStatuses'), kind: 'json', defaultValue: { version: 1, statuses: [408, 409, 429, 500, 502, 503, 504] } }, { key: 'auto_disable_policy', label: t('autoDisable'), kind: 'json', defaultValue: { version: 1, enabled: false } },]} />
      </Tabs.Panel>
    </Tabs>
  )
}
