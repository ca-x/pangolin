import { Button, Group, Modal, SimpleGrid, Stack, Text } from '@mantine/core'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { catalogCard, CARD_CAPABILITIES } from '../catalogCard'
import type { Document } from '../api'

export function ModelFacts({ row }: { row: Document }) {
  const { t, i18n } = useTranslation()
  const [open, setOpen] = useState(false)
  const card = catalogCard(row.catalog_metadata)
  const list = (value: string[] | null | undefined) => value == null ? t('factUnknown') : value.length ? value.join(', ') : t('factNone')
  const limit = (value: number | null | undefined) => value != null && value > 0 ? new Intl.NumberFormat(i18n.language).format(value) : t('factUnknown')
  const facts = [
    ...CARD_CAPABILITIES.map((name) => [t(`modelFact_${name}`), card?.capabilities?.[name] === true ? t('factSupported') : card?.capabilities?.[name] === false ? t('factUnsupported') : t('factUnknown')]),
    [t('modelFact_context'), limit(card?.limits?.context)], [t('modelFact_output'), limit(card?.limits?.output)],
    [t('modelFact_inputModalities'), list(card?.modalities?.input)], [t('modelFact_outputModalities'), list(card?.modalities?.output)], [t('modelFact_reasoningLevels'), list(card?.reasoning_levels)],
  ]
  return <>
    <Button variant="subtle" size="compact-sm" aria-label={`${t('modelFacts')} ${row.public_name}`} onClick={() => setOpen(true)}>{t('modelFacts')}</Button>
    <Modal opened={open} onClose={() => setOpen(false)} title={`${t('modelFacts')} · ${row.public_name}`} closeButtonProps={{ 'aria-label': t('close') }}>
      <Stack gap="md">
        <Text size="md" c="dimmed">{t('modelConditionalRoutes')}</Text>
        <SimpleGrid component="dl" cols={{ base: 1, sm: 2 }} m={0} spacing="md">
          {facts.map(([label, value]) => <Stack component="div" key={label} gap={2} style={{ minWidth: 0 }}><Text component="dt" size="md" c="dimmed">{label}</Text><Text component="dd" m={0} size="md" style={{ overflowWrap: 'anywhere' }}>{value}</Text></Stack>)}
        </SimpleGrid>
        <Group justify="flex-end"><Button variant="default" onClick={() => setOpen(false)}>{t('close')}</Button></Group>
      </Stack>
    </Modal>
  </>
}
