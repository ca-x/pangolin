import { Badge, Group, Stack, Tabs, Text } from '@mantine/core'
import { CheckCircle2, Circle } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { EnabledPill } from '../components'
import { AllowlistField, ProtectionTemplates } from './ProtectionTemplates'
import { ProtectionRequestPreview } from './ProtectionRequestPreview'
import { BulkToggle, ResourcePage } from './shared'
import { useActiveTabInView, useRoutedTab } from './useRoutedTab'

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

export default function PromptsPage() {
  const { t } = useTranslation()
  const [tab, setTab] = useRoutedTab(['prompts', 'protection', 'overrides'] as const, 'prompts')
  const tabsRef = useActiveTabInView(tab)
  return (
    <Tabs keepMounted={false} value={tab} onChange={setTab}>
      <Tabs.List ref={tabsRef} mb="lg" className="pm-management-tabs">
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
        <ResourcePage resource="protection" title={t('protection')} description={t('protectionDescription')} empty={t('protectionEmpty')} selectable="protection" createLabel={t('addRule')} secondaryAction={<ProtectionTemplates />} notice={<ProtectionRequestPreview />} columns={[{ key: 'name', label: t('name') }, { key: 'description', label: t('description') }, { key: 'content_pattern', label: t('pattern'), mono: true }, { key: 'action', label: t('action') }, { key: 'state', label: t('ruleState'), render: (value) => <ProtectionState value={value} /> }, { key: 'test_mode', label: t('testMode') }, { key: 'enabled', label: t('status'), render: (value) => <EnabledPill enabled={value} /> }]} mobileStatus={(row) => <Group gap="xs"><ProtectionState value={row.state} /><EnabledPill enabled={row.enabled} /></Group>} fields={[{ key: 'name', label: t('name'), required: true }, { key: 'description', label: t('description'), kind: 'textarea' }, { key: 'role_pattern', label: t('rolePattern') }, { key: 'content_pattern', label: t('contentPattern'), required: true }, { key: 'action', label: t('action'), kind: 'select', options: [{ value: 'deny', label: t('deny') }, { value: 'redact', label: t('redact') }] }, { key: 'replacement', label: t('replacement') }, { key: 'scopes', label: t('scopes'), kind: 'json', defaultValue: { version: 1 } }, { key: 'allowlist', label: t('allowlistLabel'), kind: 'json', defaultValue: [], render: props => <AllowlistField {...props} /> }, { key: 'test_mode', label: t('testMode'), kind: 'checkbox', defaultValue: false }, { key: 'enabled', label: t('enabled'), kind: 'checkbox' }, { key: 'state', label: t('ruleState'), kind: 'select', required: true, defaultValue: 'active', options: [{ value: 'active', label: t('active') }, { value: 'archived', label: t('archived') }] }]} />
        <BulkToggle resource="protection" />
      </Tabs.Panel>
      <Tabs.Panel value="overrides">
        <ResourcePage resource="channel-settings" canDelete={false} title={t('overrides')} description={t('overridesDescription')} empty={t('overridesEmpty')} columns={[{ key: 'provider_id', label: t('channelId'), mono: true }, { key: 'parameter_overrides', label: t('parameterOverrides') }, { key: 'endpoint_mappings', label: t('endpointMappings') }, { key: 'retry_statuses', label: t('retryStatuses') }]} fields={[{ key: 'provider_id', label: t('channelId'), required: true }, { key: 'endpoint_mappings', label: t('endpointMappings'), kind: 'json', defaultValue: { version: 1 } }, { key: 'model_rules', label: t('modelRules'), kind: 'json', defaultValue: { version: 1 } }, { key: 'parameter_overrides', label: t('parameterOverrides'), kind: 'json', defaultValue: { version: 1 } }, { key: 'retry_statuses', label: t('retryStatuses'), kind: 'json', defaultValue: { version: 1, statuses: [408, 409, 429, 500, 502, 503, 504] } }, { key: 'auto_disable_policy', label: t('autoDisable'), kind: 'json', defaultValue: { version: 1, enabled: false } },]} />
      </Tabs.Panel>
    </Tabs>
  )
}
