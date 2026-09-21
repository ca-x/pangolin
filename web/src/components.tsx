import { Badge, Input, Modal as MantineModal, Select, Skeleton, Stack, Text, ThemeIcon, Title, Tooltip } from '@mantine/core'
import { AlertTriangle, Check } from 'lucide-react'
import { cloneElement, isValidElement, useEffect, useId, useState, type ReactNode } from 'react'
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

export function Status({ code }: { code: number }) {
  const healthy = code < 400
  return (
    <Badge variant="light" color={healthy ? 'teal' : 'red'} leftSection={healthy ? <Check size={11} strokeWidth={3.2} /> : <AlertTriangle size={11} strokeWidth={2.8} />}>
      {code}
    </Badge>
  )
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
  const list = Array.isArray(value)
    ? value.map(String).filter(Boolean)
    : value && typeof value === 'object'
      ? Object.entries(value as Record<string, unknown>).filter(([, enabled]) => enabled === true).map(([name]) => name)
      : typeof value === 'string' && value.trim() ? value.split(',').map((item) => item.trim()).filter(Boolean) : []
  if (!list.length) return <>—</>
  const shown = list.slice(0, max)
  return (
    <span style={{ display: 'inline-flex', alignItems: 'center', gap: 4 }}>
      {shown.map((item) => <Badge key={item} variant="default" size="xs">{item}</Badge>)}
      {list.length > shown.length && <Badge variant="transparent" size="xs" c="dimmed">+{list.length - shown.length}</Badge>}
    </span>
  )
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
  return (
    <Stack aria-label="Loading" gap="xs">
      {Array.from({ length: count }, (_, index) => <Skeleton key={index} height={58} radius="md" />)}
    </Stack>
  )
}