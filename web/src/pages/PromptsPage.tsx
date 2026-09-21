import { Tabs } from '@mantine/core'
import { useTranslation } from 'react-i18next'
import { EnabledPill } from '../components'
import { ResourcePage } from './shared'

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
        <ResourcePage resource="prompts" title={t('prompts')} description={t('promptsDescription')} empty={t('promptEmpty')} createLabel={t('addPrompt')} columns={[{ key: 'name', label: t('name') }, { key: 'role', label: t('role') }, { key: 'content', label: t('content') }, { key: 'enabled', label: t('status'), render: (value) => <EnabledPill enabled={value} /> }]} fields={[{ key: 'name', label: t('name'), required: true }, { key: 'role', label: t('role'), required: true, defaultValue: 'system' }, { key: 'content', label: t('content'), kind: 'textarea', required: true }, { key: 'activation', label: t('activation'), kind: 'json', defaultValue: { version: 1 } }, { key: 'enabled', label: t('enabled'), kind: 'checkbox' }]} />
      </Tabs.Panel>
      <Tabs.Panel value="protection">
        <ResourcePage resource="protection" title={t('protection')} description={t('protectionDescription')} empty={t('protectionEmpty')} createLabel={t('addRule')} columns={[{ key: 'name', label: t('name') }, { key: 'content_pattern', label: t('pattern'), mono: true }, { key: 'action', label: t('action') }, { key: 'test_mode', label: t('testMode') }, { key: 'enabled', label: t('status'), render: (value) => <EnabledPill enabled={value} /> }]} fields={[{ key: 'name', label: t('name'), required: true }, { key: 'role_pattern', label: t('rolePattern') }, { key: 'content_pattern', label: t('contentPattern'), required: true }, { key: 'action', label: t('action'), kind: 'select', options: [{ value: 'deny', label: t('deny') }, { value: 'redact', label: t('redact') }] }, { key: 'replacement', label: t('replacement') }, { key: 'scopes', label: t('scopes'), kind: 'json', defaultValue: { version: 1 } }, { key: 'test_mode', label: t('testMode'), kind: 'checkbox', defaultValue: false }, { key: 'enabled', label: t('enabled'), kind: 'checkbox' }]} />
      </Tabs.Panel>
      <Tabs.Panel value="overrides">
        <ResourcePage resource="channel-settings" canDelete={false} title={t('overrides')} description={t('overridesDescription')} empty={t('overridesEmpty')} columns={[{ key: 'provider_id', label: t('channelId'), mono: true }, { key: 'parameter_overrides', label: t('parameterOverrides') }, { key: 'endpoint_mappings', label: t('endpointMappings') }, { key: 'retry_statuses', label: t('retryStatuses') }]} fields={[{ key: 'provider_id', label: t('channelId'), required: true }, { key: 'endpoint_mappings', label: t('endpointMappings'), kind: 'json', defaultValue: { version: 1 } }, { key: 'model_rules', label: t('modelRules'), kind: 'json', defaultValue: { version: 1 } }, { key: 'parameter_overrides', label: t('parameterOverrides'), kind: 'json', defaultValue: { version: 1 } }, { key: 'retry_statuses', label: t('retryStatuses'), kind: 'json', defaultValue: { version: 1, statuses: [408, 409, 429, 500, 502, 503, 504] } }, { key: 'auto_disable_policy', label: t('autoDisable'), kind: 'json', defaultValue: { version: 1, enabled: false } },]} />
      </Tabs.Panel>
    </Tabs>
  )
}