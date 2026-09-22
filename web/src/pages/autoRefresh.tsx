import { Button, Group, Menu } from '@mantine/core'
import { ChevronDown, RefreshCw } from 'lucide-react'
import { useCallback, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

/**
 * Live-refresh control shared by the job list and the catalog subscriptions: a
 * manual refresh next to a persisted interval choice. The choice is per view
 * (each caller passes its own storage key), so pausing one list pauses nothing
 * else, and the manual button stays usable while an interval runs.
 */
export const AUTO_REFRESH_INTERVALS = [10000, 30000, 60000, 300000] as const

export type EnabledAutoRefreshInterval = (typeof AUTO_REFRESH_INTERVALS)[number]
export type AutoRefreshInterval = EnabledAutoRefreshInterval | null

/** A manual refresh keeps its spinner up at least this long, so an instant response still reads as work. */
const MIN_SPIN_MS = 600

type AutoRefreshStorage = Pick<Storage, 'getItem' | 'setItem' | 'removeItem'>

export function formatInterval(value: number): string {
  return value >= 60000 ? `${value / 60000}m` : `${value / 1000}s`
}

/** An interval that is not one of the offered choices — including the stored "off" — reads as paused. */
export function parseAutoRefreshInterval(value: string | null): AutoRefreshInterval {
  return AUTO_REFRESH_INTERVALS.find((interval) => String(interval) === value) ?? null
}

export function readAutoRefreshInterval(storageKey: string, storage: AutoRefreshStorage): AutoRefreshInterval {
  try {
    const stored = storage.getItem(storageKey)
    const interval = parseAutoRefreshInterval(stored)
    // A choice this build no longer offers must not keep a view paused silently.
    if (stored !== null && interval === null) storage.removeItem(storageKey)
    return interval
  } catch {
    return null
  }
}

export function writeAutoRefreshInterval(storageKey: string, interval: AutoRefreshInterval, storage: AutoRefreshStorage) {
  if (interval === null) storage.removeItem(storageKey)
  else storage.setItem(storageKey, String(interval))
}

export function useAutoRefreshInterval(storageKey: string) {
  const [interval, setIntervalState] = useState<AutoRefreshInterval>(() => {
    if (typeof window === 'undefined') return null
    try {
      return readAutoRefreshInterval(storageKey, window.localStorage)
    } catch {
      return null
    }
  })
  const setInterval = useCallback((next: AutoRefreshInterval) => {
    setIntervalState(next)
    try {
      writeAutoRefreshInterval(storageKey, next, window.localStorage)
    } catch {
      // Keep the in-memory choice when localStorage is unavailable.
    }
  }, [storageKey])
  return [interval, setInterval] as const
}

/** Local clock of the last successful load, or `—` before the first one. */
export function formatUpdatedAt(updatedAt: number, language: string): string {
  return updatedAt ? new Intl.DateTimeFormat(language, { timeStyle: 'medium' }).format(new Date(updatedAt)) : '—'
}

export function AutoRefreshControl({ interval, onIntervalChange, onRefresh, disabled = false }: {
  interval: AutoRefreshInterval
  onIntervalChange: (interval: AutoRefreshInterval) => void
  onRefresh: () => unknown
  disabled?: boolean
}) {
  const { t } = useTranslation()
  const [spinning, setSpinning] = useState(false)
  const inFlight = useRef(false)
  const refresh = async () => {
    if (inFlight.current) return
    inFlight.current = true
    setSpinning(true)
    const started = Date.now()
    try {
      await onRefresh()
    } catch {
      // The view owns its own error state; a failed refresh must not disable the control.
    } finally {
      const remaining = MIN_SPIN_MS - (Date.now() - started)
      if (remaining > 0) await new Promise((resolve) => setTimeout(resolve, remaining))
      inFlight.current = false
      setSpinning(false)
    }
  }
  const paused = interval === null
  return (
    <Group gap={0} wrap="nowrap">
      <Button
        variant="default"
        size="compact-sm"
        onClick={() => void refresh()}
        disabled={disabled}
        loading={spinning}
        leftSection={<RefreshCw size={15} />}
        aria-label={t('refresh')}
      >
        {t('refresh')}
      </Button>
      <Menu position="bottom-end" withinPortal={false}>
        <Menu.Target>
          <Button
            variant="default"
            size="compact-sm"
            disabled={disabled}
            aria-label={t('autoRefresh')}
            rightSection={<ChevronDown size={15} />}
            style={{ borderLeft: 0, borderTopLeftRadius: 0, borderBottomLeftRadius: 0 }}
          >
            {paused ? t('autoRefreshOff') : formatInterval(interval)}
          </Button>
        </Menu.Target>
        <Menu.Dropdown>
          <Menu.Label>{t('autoRefresh')}</Menu.Label>
          <Menu.RadioGroup value={paused ? 'off' : String(interval)} onChange={(value) => onIntervalChange(parseAutoRefreshInterval(value))}>
            <Menu.RadioItem value="off">{t('autoRefreshOff')}</Menu.RadioItem>
            {AUTO_REFRESH_INTERVALS.map((candidate) => (
              <Menu.RadioItem key={candidate} value={String(candidate)}>{formatInterval(candidate)}</Menu.RadioItem>
            ))}
          </Menu.RadioGroup>
        </Menu.Dropdown>
      </Menu>
    </Group>
  )
}
