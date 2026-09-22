import { Alert, Badge, Button, Group, Input, Modal as MantineModal, PasswordInput, Select, Stack, Text, ThemeIcon, Title, Tooltip, type PasswordInputProps } from '@mantine/core'
import { AlertTriangle, Check } from 'lucide-react'
import { ApiFailure } from './api'
import { UNMEASURED } from './observability'
import { cloneElement, isValidElement, useEffect, useId, useState, useSyncExternalStore, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

export function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  const id = useId()
  return (
    <Input.Wrapper id={id} label={label} description={hint}>
      {isValidElement(children) ? cloneElement(children as React.ReactElement<{ id?: string }>, { id }) : children}
    </Input.Wrapper>
  )
}

export function Modal({ open, onOpenChange, title, description, trigger, children }: { open?: boolean; onOpenChange?: (open: boolean) => void; title: string; description?: string; trigger?: ReactNode; children: ReactNode }) {
  const { t } = useTranslation()
  const [internalOpen, setInternalOpen] = useState(false)
  const controlled = open !== undefined
  const opened = controlled ? (open as boolean) : internalOpen
  const handleOpen = () => {
    if (onOpenChange) onOpenChange(true)
    if (!controlled) setInternalOpen(true)
  }
  const handleClose = () => {
    if (onOpenChange) onOpenChange(false)
    if (!controlled) setInternalOpen(false)
  }
  return (
    <>
      {trigger && !controlled && cloneElement(trigger as React.ReactElement<{ onClick?: React.MouseEventHandler }>, { onClick: (e: React.MouseEvent) => { handleOpen(); (trigger as React.ReactElement<{ onClick?: React.MouseEventHandler }>).props.onClick?.(e) } })}
      <MantineModal opened={opened} onClose={handleClose} title={title} closeButtonProps={{ 'aria-label': t('close') }} returnFocus size="md">
        {description ? <Text size="sm" c="dimmed" mb="md">{description}</Text> : <Text className="sr-only">{title}</Text>}
        {children}
      </MantineModal>
    </>
  )
}

/**
 * Destructive actions ask through one shared dialog. `window.confirm` cannot
 * name the object, cannot show a pending state and cannot report a failure, so
 * the ceremony lives here: call `confirmAction` from the handler that owns the
 * mutation, and mount `<ConfirmHost />` once at the application root.
 *
 * The host is a module-level store rather than a context because the call sites
 * are event handlers, including ones built inside `useMemo` column definitions
 * and ones passed down to child components. Threading a dialog element through
 * those would put five near-identical modal blocks back in the pages.
 */
type ConfirmRequest = {
  id: number
  title: string
  body: string
  confirmLabel?: string
  onConfirm: () => unknown
  trigger: HTMLElement | null
}

let activeRequest: ConfirmRequest | null = null
let mountedHosts = 0
let requestSequence = 0
const confirmListeners = new Set<() => void>()

const publishConfirm = () => { for (const listener of confirmListeners) listener() }
const subscribeConfirm = (listener: () => void) => { confirmListeners.add(listener); return () => { confirmListeners.delete(listener) } }
const readConfirm = () => activeRequest

/**
 * The record's own conflict codes, named in the active language. The server's
 * message stays on the line because that is where the colliding value lives — a
 * generic sentence alone would be less useful, not more localized.
 */
const CONFLICT_CODE_KEYS: Record<string, string> = {
  duplicate_model: 'conflictDuplicateModel',
  resource_in_use: 'conflictResourceInUse',
  history_retained: 'conflictHistoryRetained',
  archive_required: 'conflictArchiveRequired',
}

/** The sentence a failed action shows, resolved through `t`. */
export function describeFailure(error: unknown, t: (key: string) => string): string {
  const message = error instanceof Error ? error.message : String(error)
  const key = error instanceof ApiFailure && error.code ? CONFLICT_CODE_KEYS[error.code] : undefined
  return key ? `${t(key)} — ${message}` : message
}

export function confirmAction({ title, body, confirmLabel, onConfirm }: { title: string; body: string; confirmLabel?: string; onConfirm: () => unknown }) {
  if (mountedHosts === 0) throw new Error('confirmAction needs a mounted <ConfirmHost />: refusing to run a destructive action that was never confirmed')
  // The trigger tracking the edit dialogs do with `lastTrigger`, done once for
  // every caller: whoever had focus when the action was requested gets it back.
  const active = typeof document === 'undefined' ? null : document.activeElement
  activeRequest = { id: (requestSequence += 1), title, body, confirmLabel, onConfirm, trigger: active instanceof HTMLElement ? active : null }
  publishConfirm()
}

export function ConfirmHost() {
  const request = useSyncExternalStore(subscribeConfirm, readConfirm)
  useEffect(() => {
    mountedHosts += 1
    // A host that goes away (a test, or a remount) must not leave a request
    // behind for the next host to open unasked.
    return () => { mountedHosts -= 1; activeRequest = null }
  }, [])
  return request ? <ConfirmDialog key={request.id} request={request} /> : null
}

function ConfirmDialog({ request }: { request: ConfirmRequest }) {
  const { t } = useTranslation()
  const [pending, setPending] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const dismiss = () => {
    activeRequest = null
    publishConfirm()
    const { trigger } = request
    // The dialog unmounts on close, so Mantine's own focus return cannot run;
    // the trigger is focused explicitly, as the edit dialogs do.
    requestAnimationFrame(() => { if (trigger?.isConnected) trigger.focus({ preventScroll: true }) })
  }
  // Escape, an outside click and the header close button all arrive here. While
  // the mutation is in flight they are ignored, so a slow delete cannot be
  // hidden mid-flight nor fired a second time.
  const close = () => { if (!pending) dismiss() }
  const run = async () => {
    if (pending) return
    setPending(true)
    setError(null)
    try {
      await request.onConfirm()
      dismiss()
    } catch (cause) {
      setError(describeFailure(cause, t))
      setPending(false)
    }
  }
  return (
    <Modal open onOpenChange={(open) => { if (!open) close() }} title={request.title} description={request.body}>
      <Stack gap="md">
        {error && <Alert variant="light" color="red" icon={<AlertTriangle size={17} />} role="alert">{error}</Alert>}
        <Group justify="flex-end" gap="xs">
          {/* Cancel comes first and takes focus, so the safe choice is the default one. */}
          <Button variant="default" data-autofocus disabled={pending} onClick={close}>{t('cancel')}</Button>
          <Button color="red" loading={pending} onClick={() => void run()}>{request.confirmLabel || t('delete')}</Button>
        </Group>
      </Stack>
    </Modal>
  )
}

export function SelectField({ value, onValueChange, label, options, name }: { value: string; onValueChange: (value: string) => void; label: string; options: Array<{ value: string; label: string }>; name?: string }) {
  // An enum select with no selection submits nothing, which makes a form whose
  // field is required fail on the default Save. Adopt the first option through
  // the caller's state rather than only displaying it, so a submit button gated
  // on that state enables once options arrive.
  const selected = value || options[0]?.value || ''
  useEffect(() => {
    if (!value && options[0]) onValueChange(options[0].value)
  }, [value, options, onValueChange])
  return (
    <Select
      name={name}
      label={label}
      value={selected}
      onChange={(next) => { if (next !== null) onValueChange(next) }}
      data={options}
      allowDeselect={false}
    />
  )
}

export function Tip({ label, children }: { label: string; children: ReactNode }) {
  return <Tooltip label={label}>{children}</Tooltip>
}

/**
 * The upstream status of one request. A request whose status was never observed
 * reads `—`: `0` is not an HTTP status, and a green pill would claim a success
 * nobody measured. Only a status the gateway actually saw gets a badge.
 */
export function Status({ code }: { code: number | null }) {
  if (code == null) return <Text size="sm" c="dimmed">{UNMEASURED}</Text>
  const healthy = code < 400
  return (
    <Badge variant="light" color={healthy ? 'teal' : 'red'} className="status-badge" leftSection={healthy ? <Check size={11} strokeWidth={3.2} /> : <AlertTriangle size={11} strokeWidth={2.8} />}>
      {code}
    </Badge>
  )
}

/**
 * A password field whose visibility toggle is named in the active language.
 * Mantine's own default is the hard-coded English "Toggle password visibility",
 * which was the last English string in the zh-CN console — an accessible name is
 * copy like any other. Callers may still override the toggle's own props.
 */
export function SecretInput(props: PasswordInputProps) {
  const { t } = useTranslation()
  return <PasswordInput {...props} visibilityToggleButtonProps={{ 'aria-label': t('togglePasswordVisibility'), ...props.visibilityToggleButtonProps }} />
}

export function EnabledPill({ enabled, tone }: { enabled: unknown; tone?: 'accent' | 'warning' }) {
  const { t } = useTranslation()
  const on = Boolean(Number(enabled))
  const color = on ? (tone === 'warning' ? 'yellow' : tone === 'accent' ? 'pangolin' : 'teal') : 'gray'
  return (
    <Badge variant="light" color={color} leftSection={<span aria-hidden="true" style={{ width: 6, height: 6, borderRadius: '50%', background: 'currentColor' }} />}>
      {on ? t('enabled') : t('disabled')}
    </Badge>
  )
}

export function CapabilityTags({ value, max = 3 }: { value: unknown; max?: number }) {
  const label = useCapabilityLabel()
  const list = capabilityList(value)
  if (!list.length) return <>—</>
  const shown = list.slice(0, max)
  // Mantine's smallest badge is 9px, upper-cased and ellipsised, which turned
  // every capability into a clipped pill. These are reference values, so they
  // read as text: 12.5px, sentence case, never truncated mid-word.
  return (
    <span className="pm-capabilities">
      {shown.map((item) => <span key={item} className="pm-capability">{label(item)}</span>)}
      {list.length > shown.length && <span className="pm-capability pm-capability-more">+{list.length - shown.length}</span>}
    </span>
  )
}

/**
 * The capability identifiers the record system stores, as i18n keys. A name the
 * console does not know is data — a capability a newer backend added — so it is
 * shown verbatim rather than hidden behind a key that does not exist.
 */
const CAPABILITY_KEYS: Record<string, string> = {
  chat: 'capabilityChat', completions: 'capabilityCompletions', responses: 'capabilityResponses', messages: 'capabilityMessages',
  embeddings: 'capabilityEmbeddings', moderations: 'capabilityModerations', search: 'capabilitySearch', images: 'capabilityImages',
  videos: 'capabilityVideos', speech: 'capabilitySpeech', transcriptions: 'capabilityTranscriptions', translations: 'capabilityTranslations',
  rerank: 'capabilityRerank', gemini: 'capabilityGemini',
}

/** Capabilities as a list, from the array, object-map or comma-separated form the record system stores. */
export function capabilityList(value: unknown): string[] {
  if (Array.isArray(value)) return value.map(String).filter(Boolean)
  if (value && typeof value === 'object') return Object.entries(value as Record<string, unknown>).filter(([, enabled]) => enabled === true).map(([name]) => name)
  return typeof value === 'string' && value.trim() ? value.split(',').map((item) => item.trim()).filter(Boolean) : []
}

/** One capability name in the active language. */
export function useCapabilityLabel() {
  const { t } = useTranslation()
  return (name: string) => CAPABILITY_KEYS[name] ? t(CAPABILITY_KEYS[name]) : name
}

export function EmptyState({ icon, title, copy, action }: { icon: ReactNode; title: string; copy: string; action?: ReactNode }) {
  return (
    <Stack align="center" gap="md" py="xl" style={{ border: '1px dashed var(--mantine-color-default-border)', borderRadius: 'var(--mantine-radius-default)' }}>
      <ThemeIcon variant="light" size={54} radius="lg">{icon}</ThemeIcon>
      <Title order={3}>{title}</Title>
      <Text size="sm" c="dimmed" maw={420} ta="center">{copy}</Text>
      {action}
    </Stack>
  )
}

export function SkeletonRows({ count = 4 }: { count?: number }) {
  const { t } = useTranslation()
  // The sweep lives on the element's own layer as a transform (`.skeleton::after`),
  // so the placeholder never repaints: see styles.css and the motion rules in
  // AGENTS.md. The name is localized like every other visible string.
  return (
    <Stack className="skeleton-stack" role="status" aria-label={t('loading')} gap="xs">
      {Array.from({ length: count }, (_, index) => <div key={index} className="skeleton" />)}
    </Stack>
  )
}

/**
 * The error state of an auxiliary query — a picker's option list, a name lookup.
 * Such a query feeds a control rather than the page, so a failed read used to
 * leave an empty or misleading control behind. This names the failure and offers
 * the retry in place, next to whatever the value is needed for.
 */
export function InlineQueryError({ message, onRetry }: { message: string; onRetry?: () => void }) {
  const { t } = useTranslation()
  return <Alert variant="light" color="red" radius="md" py={6} role="alert" icon={<AlertTriangle size={17} />} className="inline-query-error"><Group gap="sm" justify="space-between" align="center" wrap="nowrap"><Text size="sm">{message}</Text>{onRetry && <Button variant="outline" color="red" size="compact-xs" onClick={onRetry}>{t('retry')}</Button>}</Group></Alert>
}
