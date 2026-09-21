import { Alert, Button, Center, Group, Paper, PasswordInput, Stack, Text, TextInput, Title } from '@mantine/core'
import { useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useNavigate, useSearchParams } from 'react-router'
import { toast } from 'sonner'
import { api } from './api'

export function Setup() {
  const { t, i18n } = useTranslation()
  const navigate = useNavigate()
  const [busy, setBusy] = useState(false)
  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault(); setBusy(true)
    const data = new FormData(event.currentTarget)
    try {
      await api('/api/v1/setup', { method: 'POST', body: JSON.stringify({ instance_name: data.get('instance_name'), email: data.get('email'), password: data.get('password'), language: i18n.language }) })
      navigate('/'); location.reload()
    } catch (error) { toast.error(error instanceof Error ? error.message : t('formError')); setBusy(false) }
  }
  return (
    <AuthFrame>
      <Paper p="xl" withBorder radius="xl" shadow="lg">
        <form onSubmit={submit}>
          <Stack gap="md">
            <BrandHero />
            <div>
              <Title order={1}>{t('setupTitle')}</Title>
              <Text size="sm" c="dimmed">{t('setupIntro')}</Text>
            </div>
            <TextInput name="instance_name" label={t('instanceName')} defaultValue={t('product')} required />
            <TextInput name="email" type="email" label={t('email')} autoComplete="email" required autoFocus />
            <PasswordInput name="password" label={t('password')} description={t('passwordHint')} minLength={12} autoComplete="new-password" required />
            <Button type="submit" fullWidth disabled={busy}>{busy ? t('loading') : t('createAdmin')}</Button>
          </Stack>
        </form>
      </Paper>
    </AuthFrame>
  )
}

export function Login() {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const [busy, setBusy] = useState(false)
  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault(); setBusy(true)
    const data = new FormData(event.currentTarget)
    try { await api('/api/v1/auth/login', { method: 'POST', body: JSON.stringify({ email: data.get('email'), password: data.get('password') }) }); navigate('/'); location.reload() }
    catch (error) { toast.error(error instanceof Error ? error.message : t('networkError')); setBusy(false) }
  }
  return (
    <AuthFrame>
      <Paper p="xl" withBorder radius="xl" shadow="lg">
        <form onSubmit={submit}>
          <Stack gap="md">
            <BrandHero />
            <div>
              <Title order={1}>{t('loginTitle')}</Title>
              <Text size="sm" c="dimmed">{t('loginIntro')}</Text>
            </div>
            <TextInput name="email" type="email" label={t('email')} autoComplete="email" required autoFocus />
            <PasswordInput name="password" label={t('password')} autoComplete="current-password" required />
            <Button type="submit" fullWidth disabled={busy}>{busy ? t('loading') : t('signIn')}</Button>
          </Stack>
        </form>
      </Paper>
    </AuthFrame>
  )
}

type Invitation = { email: string; project_id: string; role_id: string; expires_at: number }

export function InvitationAccept({ authenticated }: { authenticated: boolean }) {
  const { t, i18n } = useTranslation()
  const navigate = useNavigate()
  const [params] = useSearchParams()
  const token = params.get('token') || ''
  const [invitation, setInvitation] = useState<Invitation | null>(null)
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)

  const inspect = async () => {
    setError('')
    try { setInvitation(await api<Invitation>('/api/v1/invitations/inspect', { method: 'POST', body: JSON.stringify({ token }) })) }
    catch (reason) { setError(reason instanceof Error ? reason.message : t('invitationInvalid')) }
  }
  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault(); setBusy(true)
    const data = new FormData(event.currentTarget)
    try {
      await api('/api/v1/invitations/accept', { method: 'POST', body: JSON.stringify({ token, password: authenticated ? null : data.get('password'), display_name: data.get('display_name') || null, language: i18n.language }) })
      navigate('/'); location.reload()
    } catch (reason) { setError(reason instanceof Error ? reason.message : t('formError')); setBusy(false) }
  }

  if (!token) return (
    <AuthFrame>
      <Paper p="xl" withBorder radius="xl" shadow="lg">
        <Stack gap="md">
          <BrandHero />
          <Title order={1}>{t('invitationInvalid')}</Title>
          <Button component={Link} to="/login" variant="default">{t('backToLogin')}</Button>
        </Stack>
      </Paper>
    </AuthFrame>
  )

  if (!invitation) return (
    <AuthFrame>
      <Paper p="xl" withBorder radius="xl" shadow="lg">
        <Stack gap="md">
          <BrandHero />
          <Title order={1}>{t('acceptInvitation')}</Title>
          {error && <Alert variant="light" color="red" role="alert">{error}</Alert>}
          <Button onClick={() => void inspect()}>{t('inspectInvitation')}</Button>
        </Stack>
      </Paper>
    </AuthFrame>
  )

  return (
    <AuthFrame>
      <Paper p="xl" withBorder radius="xl" shadow="lg">
        <form onSubmit={submit}>
          <Stack gap="md">
            <BrandHero />
            <div>
              <Title order={1}>{t('acceptInvitation')}</Title>
              <Text size="sm" c="dimmed">{t('invitedAs', { email: invitation.email })}</Text>
            </div>
            {!authenticated && <>
              <TextInput name="display_name" label={t('displayName')} required />
              <PasswordInput name="password" label={t('password')} description={t('passwordHint')} minLength={12} autoComplete="new-password" required />
            </>}
            {error && <Alert variant="light" color="red" role="alert">{error}</Alert>}
            <Button type="submit" fullWidth disabled={busy}>{busy ? t('loading') : t('acceptInvitation')}</Button>
          </Stack>
        </form>
      </Paper>
    </AuthFrame>
  )
}

function AuthFrame({ children }: { children: React.ReactNode }) {
  return (
    <Center h="100vh" pos="relative" style={{ overflow: 'hidden' }}>
      {/* Decorative accent panel */}
      <div aria-hidden="true" style={{
        position: 'absolute', inset: 0, opacity: 0.55,
        background: 'var(--mantine-primary-color-light)',
        clipPath: 'polygon(0 0, 26% 0, 8% 100%, 0 100%)',
        pointerEvents: 'none',
      }} />
      <div style={{ position: 'relative', width: 'min(440px, 100%)', padding: 'var(--mantine-spacing-md)' }}>
        {children}
      </div>
    </Center>
  )
}

function BrandHero() {
  const { t } = useTranslation()
  return (
    <Group gap="sm" c="dimmed" fw={600}>
      <img src="/logo.webp" alt="" width={42} height={42} style={{ objectFit: 'contain' }} />
      <span>{t('product')}</span>
    </Group>
  )
}