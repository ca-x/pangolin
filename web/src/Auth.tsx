import { Alert, Button, Center, Group, Paper, Stack, Text, TextInput, Title } from '@mantine/core'
import { useEffect, useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useNavigate, useSearchParams } from 'react-router'
import { toast } from 'sonner'
import { api } from './api'
import { SecretInput } from './components'
import { ProviderIcon } from './ProviderIcon'

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
            <SecretInput name="password" label={t('password')} description={t('passwordHint')} minLength={12} autoComplete="new-password" required />
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
  const [oidcProviders, setOidcProviders] = useState<PublicOidcProvider[]>([])
  const [oidcDiscovery, setOidcDiscovery] = useState<'loading' | 'success' | 'failure'>('loading')
  const [oidcBusy, setOidcBusy] = useState<string | null>(null)
  const [oidcError, setOidcError] = useState('')
  useEffect(() => {
    let current = true
    void api<PublicOidcProvider[]>('/api/v1/auth/oidc/providers')
      .then((providers) => {
        if (current) {
          setOidcProviders(Array.isArray(providers) ? providers : [])
          setOidcDiscovery('success')
        }
      })
      .catch(() => {
        // OIDC discovery is best effort; local password sign-in remains primary.
        if (current) setOidcDiscovery('failure')
      })
    return () => { current = false }
  }, [])
  const startOidc = async (provider: PublicOidcProvider) => {
    setOidcBusy(provider.id)
    setOidcError('')
    try {
      const result = await api<OidcStartResponse>(`/api/v1/auth/oidc/${encodeURIComponent(provider.id)}/start`)
      location.assign(validAuthorizationUrl(result.authorization_url, t('oidcStartInvalid')))
    } catch (error) {
      setOidcError(error instanceof Error ? error.message : t('networkError'))
      setOidcBusy(null)
    }
  }
  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault(); setBusy(true)
    const data = new FormData(event.currentTarget)
    try { await api('/api/v1/auth/login', { method: 'POST', body: JSON.stringify({ email: data.get('email'), password: data.get('password') }) }); navigate('/'); location.reload() }
    catch (error) { toast.error(error instanceof Error ? error.message : t('networkError')); setBusy(false) }
  }
  const providerOnly = oidcDiscovery === 'success' && oidcProviders.length > 0 && oidcProviders.every((provider) => provider.login_only)
  return (
    <AuthFrame>
      <Paper p="xl" withBorder radius="xl" shadow="lg">
        <Stack gap="md">
          <BrandHero />
          <div>
            <Title order={1}>{t('loginTitle')}</Title>
            <Text size="sm" c="dimmed">{t('loginIntro')}</Text>
          </div>
          {!providerOnly && <form onSubmit={submit}>
            <Stack gap="md">
              <TextInput name="email" type="email" label={t('email')} autoComplete="email" required autoFocus />
              <SecretInput name="password" label={t('password')} autoComplete="current-password" required />
              <Button type="submit" fullWidth disabled={busy}>{busy ? t('loading') : t('signIn')}</Button>
            </Stack>
          </form>}
          {oidcProviders.length > 0 && (
            <Stack gap="xs" role="group" aria-label={t('oidcSignInOptions')}>
              <Text size="sm" c="dimmed" ta="center">{t('oidcSignInOptions')}</Text>
              {oidcError && <Alert variant="light" color="red" role="alert">{oidcError}</Alert>}
              {oidcProviders.map((provider) => {
                const textColor = buttonTextColor(provider.button_color)
                const branded = textColor && provider.button_color
                  ? { backgroundColor: provider.button_color, borderColor: provider.button_color, color: textColor }
                  : undefined
                return <Button
                    key={provider.id}
                    aria-label={t('continueWithProvider', { provider: provider.display_name })}
                    type="button"
                    onClick={() => void startOidc(provider)}
                    loading={oidcBusy === provider.id}
                    disabled={oidcBusy !== null && oidcBusy !== provider.id}
                    variant="default"
                    fullWidth
                    styles={{
                      root: { height: 'auto', minHeight: 40, paddingBlock: 8, ...branded },
                      label: { whiteSpace: 'normal', overflowWrap: 'anywhere', lineHeight: 1.4 },
                    }}
                  >
                    <Group gap="xs" wrap="nowrap" justify="center" style={{ minWidth: 0 }}>
                      <span aria-hidden="true"><ProviderIcon logoKey={provider.logo_key ?? undefined} name={provider.display_name} size={24} /></span>
                      <span style={{ minWidth: 0, overflowWrap: 'anywhere' }}>{t('continueWithProvider', { provider: provider.display_name })}</span>
                    </Group>
                  </Button>
              })}
            </Stack>
          )}
        </Stack>
      </Paper>
    </AuthFrame>
  )
}

type PublicOidcProvider = { id: string; display_name: string; button_color: string | null; logo_key: string | null; login_only: boolean }
type OidcStartResponse = { authorization_url: string; expires_in?: number }

export function buttonTextColor(background: string | null | undefined): '#000000' | '#FFFFFF' | undefined {
  if (!background || !/^#[0-9a-f]{6}$/i.test(background)) return undefined
  const channels = [1, 3, 5].map((offset) => Number.parseInt(background.slice(offset, offset + 2), 16) / 255)
  const [red, green, blue] = channels.map((channel) => channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4)
  const luminance = 0.2126 * red + 0.7152 * green + 0.0722 * blue
  const blackContrast = (luminance + 0.05) / 0.05
  const whiteContrast = 1.05 / (luminance + 0.05)
  return blackContrast >= whiteContrast ? '#000000' : '#FFFFFF'
}

export function validAuthorizationUrl(raw: unknown, invalidMessage = 'OIDC start did not return a valid authorization URL.'): string {
  if (typeof raw !== 'string' || !raw.trim()) throw new Error(invalidMessage)
  let parsed: URL
  try { parsed = new URL(raw) }
  catch { throw new Error(invalidMessage) }
  if (!['http:', 'https:'].includes(parsed.protocol)) throw new Error(invalidMessage)
  return parsed.toString()
}

/** Self-service linking is deliberately separate from login: the POST start
 * endpoint captures the current session user in durable one-time state. */
export function OidcLinkPanel({ navigate = (url) => window.location.assign(url) }: { navigate?: (url: string) => void }) {
  const { t } = useTranslation()
  const [providers, setProviders] = useState<PublicOidcProvider[] | null>(null)
  const [readError, setReadError] = useState('')
  const [startError, setStartError] = useState('')
  const [busy, setBusy] = useState('')
  const [attempt, setAttempt] = useState(0)
  useEffect(() => {
    let current = true
    setProviders(null)
    setReadError('')
    void api<PublicOidcProvider[]>('/api/v1/auth/oidc/providers')
      .then((rows) => { if (current) setProviders(Array.isArray(rows) ? rows : []) })
      .catch((error) => { if (current) setReadError(error instanceof Error ? error.message : t('networkError')) })
    return () => { current = false }
  }, [attempt, t])
  const start = async (provider: PublicOidcProvider) => {
    setBusy(provider.id)
    setStartError('')
    try {
      const result = await api<OidcStartResponse>(`/api/v1/auth/oidc/${encodeURIComponent(provider.id)}/link/start`, { method: 'POST', body: '{}' })
      navigate(validAuthorizationUrl(result.authorization_url, t('oidcStartInvalid')))
    } catch (error) {
      setStartError(error instanceof Error ? error.message : t('networkError'))
      setBusy('')
    }
  }
  return (
    <Paper withBorder p="lg">
      <Stack gap="md">
        <div><Title order={2}>{t('linkedSignIn')}</Title><Text size="sm" c="dimmed" mt={4}>{t('linkedSignInHint')}</Text></div>
        {providers === null && !readError && <Text size="sm" c="dimmed" role="status">{t('loading')}</Text>}
        {readError && <Alert color="red" role="alert" title={t('oidcProvidersUnavailable')}><Stack gap="sm"><Text size="sm">{readError}</Text><Button variant="outline" color="red" onClick={() => setAttempt((value) => value + 1)}>{t('retry')}</Button></Stack></Alert>}
        {providers?.length === 0 && <Text size="sm" c="dimmed">{t('oidcLinkEmpty')}</Text>}
        {startError && <Alert color="red" role="alert">{startError}</Alert>}
        {providers?.map((provider) => <Button key={provider.id} variant="default" loading={busy === provider.id} disabled={Boolean(busy) && busy !== provider.id} onClick={() => void start(provider)} styles={{ label: { whiteSpace: 'normal', overflowWrap: 'anywhere' } }}>{t('linkWithProvider', { provider: provider.display_name })}</Button>)}
      </Stack>
    </Paper>
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
              <SecretInput name="password" label={t('password')} description={t('passwordHint')} minLength={12} autoComplete="new-password" required />
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
