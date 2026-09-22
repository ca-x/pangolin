import { Stack } from '@mantine/core'
import { useTranslation } from 'react-i18next'
import { OidcLinkPanel } from '../Auth'
import { PageHeader } from './shared'

export default function AccountPage() {
  const { t } = useTranslation()
  return (
    <Stack gap="lg">
      <PageHeader title={t('account')} description={t('accountDescription')} />
      <OidcLinkPanel />
    </Stack>
  )
}
