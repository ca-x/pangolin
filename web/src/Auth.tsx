import { useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'
import { toast } from 'sonner'
import { api } from './api'
import { Field } from './components'

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
  return <AuthFrame><form className="auth-card" onSubmit={submit}><BrandHero /><div><h1>{t('setupTitle')}</h1><p>{t('setupIntro')}</p></div><Field label={t('instanceName')}><input name="instance_name" defaultValue={t('product')} required /></Field><Field label={t('email')}><input name="email" type="email" autoComplete="email" required autoFocus /></Field><Field label={t('password')} hint={t('passwordHint')}><input name="password" type="password" minLength={12} autoComplete="new-password" required /></Field><button className="button button-primary" disabled={busy}>{busy ? t('loading') : t('createAdmin')}</button></form></AuthFrame>
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
  return <AuthFrame><form className="auth-card" onSubmit={submit}><BrandHero /><div><h1>{t('loginTitle')}</h1><p>{t('loginIntro')}</p></div><Field label={t('email')}><input name="email" type="email" autoComplete="email" required autoFocus /></Field><Field label={t('password')}><input name="password" type="password" autoComplete="current-password" required /></Field><button className="button button-primary" disabled={busy}>{busy ? t('loading') : t('signIn')}</button></form></AuthFrame>
}

function AuthFrame({ children }: { children: React.ReactNode }) { return <main className="auth-layout"><div className="auth-material" aria-hidden="true" /><div className="auth-wrap">{children}</div></main> }
function BrandHero() { const { t } = useTranslation(); return <div className="auth-brand"><img src="/logo.webp" alt="" /><span>{t('product')}</span></div> }
