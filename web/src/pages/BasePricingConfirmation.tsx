import { Switch } from '@mantine/core'
import { useLayoutEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
/** Existing base rates are preserved; nonzero rates cannot be marked unconfigured. */
export function BasePricingConfirmation({ name, value }: { name: string; value: unknown }) {
  const { t } = useTranslation()
  const ref = useRef<HTMLDivElement>(null)
  const [confirmed, setConfirmed] = useState(value === true || value === 1)
  const [nonzero, setNonzero] = useState(false)
  useLayoutEffect(() => {
    const form = ref.current?.closest('form')
    if (!form) return
    const update = () => {
      const data = new FormData(form)
      setNonzero(Number(data.get('input_price_micros')) > 0 || Number(data.get('output_price_micros')) > 0)
    }
    update()
    form.addEventListener('input', update)
    form.addEventListener('change', update)
    return () => {
      form.removeEventListener('input', update)
      form.removeEventListener('change', update)
    }
  }, [])
  return (
    <div ref={ref}>
      <Switch
        aria-label={t('pricingConfirmBase')}
        label={t('pricingConfirmBase')}
        description={t('pricingBaseHelp')}
        checked={nonzero || confirmed}
        disabled={nonzero}
        onChange={(event) => setConfirmed(event.currentTarget.checked)}
      />
      <input type="hidden" name={name} value={nonzero || confirmed ? 'on' : ''} />
    </div>
  )
}
