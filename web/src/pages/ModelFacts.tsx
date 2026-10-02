import { Button, Group, Modal, SimpleGrid, Stack, Text } from '@mantine/core'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { Document } from '../api'

type Metadata = { capabilities?: Record<string, boolean | null>; limits?: { context?: number | null; output?: number | null }; modalities?: { input?: string[] | null; output?: string[] | null }; reasoning_levels?: string[] | null }
const capabilities = ['streaming', 'tools', 'reasoning', 'temperature', 'vision', 'json_schema', 'web_search', 'file_search', 'computer_use', 'caching', 'batch'] as const
export function ModelFacts({ row }: { row: Document }) {
  const { t, i18n } = useTranslation()
  const [open, setOpen] = useState(false)
  const card = (row.catalog_metadata || {}) as Metadata
  const list = (value: string[] | null | undefined) => value == null ? t('factUnknown') : value.length ? value.join(', ') : t('factNone')
  const limit = (value: number | null | undefined) => value != null && value > 0 ? new Intl.NumberFormat(i18n.language).format(value) : t('factUnknown')
  const facts = [
    ...capabilities.map((name) => [t(`modelFact_${name}`), card.capabilities?.[name] === true ? t('factSupported') : card.capabilities?.[name] === false ? t('factUnsupported') : t('factUnknown')]),
    [t('modelFact_context'), limit(card.limits?.context)], [t('modelFact_output'), limit(card.limits?.output)],
    [t('modelFact_inputModalities'), list(card.modalities?.input)], [t('modelFact_outputModalities'), list(card.modalities?.output)], [t('modelFact_reasoningLevels'), list(card.reasoning_levels)],
  ]
  return <>
    <Button variant="subtle" size="compact-sm" aria-label={`${t('modelFacts')} ${row.public_name}`} onClick={() => setOpen(true)}>{t('modelFacts')}</Button>
    <Modal opened={open} onClose={() => setOpen(false)} title={`${t('modelFacts')} · ${row.public_name}`} closeButtonProps={{ 'aria-label': t('close') }}>
      <Stack gap="md">
        <Text size="sm" c="dimmed">{t('modelConditionalRoutes')}</Text>
        <SimpleGrid component="dl" cols={{ base: 1, sm: 2 }} m={0} spacing="md">
          {facts.map(([label, value]) => <Stack component="div" key={label} gap={2} style={{ minWidth: 0 }}><Text component="dt" size="sm" c="dimmed">{label}</Text><Text component="dd" m={0} size="sm" style={{ overflowWrap: 'anywhere' }}>{value}</Text></Stack>)}
        </SimpleGrid>
        <Group justify="flex-end"><Button variant="default" onClick={() => setOpen(false)}>{t('close')}</Button></Group>
      </Stack>
    </Modal>
  </>
}
