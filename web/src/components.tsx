import * as Dialog from '@radix-ui/react-dialog'
import * as Select from '@radix-ui/react-select'
import { Check, ChevronDown, X } from 'lucide-react'
import { useEffect, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

export function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return <label className="field"><span className="field-label">{label}</span>{children}{hint && <span className="field-hint">{hint}</span>}</label>
}

export function Modal({ open, onOpenChange, title, description, trigger, children }: { open?: boolean; onOpenChange?: (open: boolean) => void; title: string; description?: string; trigger?: ReactNode; children: ReactNode }) {
  const { t } = useTranslation()
  return <Dialog.Root open={open} onOpenChange={onOpenChange}>
    {trigger && <Dialog.Trigger asChild>{trigger}</Dialog.Trigger>}
    <Dialog.Portal>
      <Dialog.Overlay className="dialog-overlay" />
      <Dialog.Content className="dialog-content">
        <div className="dialog-heading"><div><Dialog.Title>{title}</Dialog.Title>{description ? <Dialog.Description>{description}</Dialog.Description> : <Dialog.Description className="sr-only">{title}</Dialog.Description>}</div><Dialog.Close className="icon-button" aria-label={t('close')}><X size={18} /></Dialog.Close></div>
        {children}
      </Dialog.Content>
    </Dialog.Portal>
  </Dialog.Root>
}

export function SelectField({ value, onValueChange, label, options, name }: { value: string; onValueChange: (value: string) => void; label: string; options: Array<{ value: string; label: string }>; name?: string }) {
  const [internal,setInternal]=useState(value)
  useEffect(()=>setInternal(value),[value])
  return <Field label={label}><Select.Root name={name} value={internal} onValueChange={(next)=>{setInternal(next);onValueChange(next)}}><Select.Trigger className="select-trigger"><Select.Value /><Select.Icon><ChevronDown size={16} /></Select.Icon></Select.Trigger><Select.Portal><Select.Content className="select-content" position="popper" sideOffset={6}><Select.Viewport>{options.map((option) => <Select.Item className="select-item" key={option.value} value={option.value}><Select.ItemText>{option.label}</Select.ItemText><Select.ItemIndicator><Check size={15} /></Select.ItemIndicator></Select.Item>)}</Select.Viewport></Select.Content></Select.Portal></Select.Root></Field>
}

export function Status({ code }: { code: number }) {
  const healthy = code < 400
  return <span className={`status ${healthy ? 'status-good' : 'status-bad'}`}><span aria-hidden="true">{healthy ? '✓' : '!'}</span>{code}</span>
}

export function EmptyState({ icon, title, copy, action }: { icon: ReactNode; title: string; copy: string; action?: ReactNode }) {
  return <div className="empty-state"><span className="empty-icon" aria-hidden="true">{icon}</span><h3>{title}</h3><p>{copy}</p>{action}</div>
}

export function SkeletonRows({ count = 4 }: { count?: number }) {
  return <div className="skeleton-stack" aria-label="Loading">{Array.from({ length: count }, (_, index) => <div className="skeleton" key={index} />)}</div>
}
