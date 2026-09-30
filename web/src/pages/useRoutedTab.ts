import { useLayoutEffect, useRef } from 'react'
import { useSearchParams } from 'react-router'

/** Keep a management view in the URL so sidebar links, refresh and browser back agree. */
export function useRoutedTab<const T extends string>(values: readonly T[], fallback: T, legacyTab?: string) {
  const [params, setParams] = useSearchParams()
  const requested = params.get('tab') ?? legacyTab
  const tab = values.find((value) => value === requested) ?? fallback
  const setTab = (next: string | null, replace = false) => {
    if (!next || !values.some((value) => value === next)) return
    const updated = new URLSearchParams(params)
    if (next === fallback) updated.delete('tab')
    else updated.set('tab', next)
    setParams(updated, { replace })
  }
  return [tab, setTab] as const
}

/** Reveal the selected tab on narrow screens without scrolling the whole page. */
export function useActiveTabInView(active: string | undefined) {
  const listRef = useRef<HTMLDivElement>(null)
  useLayoutEffect(() => {
    const list = listRef.current
    if (!list) return
    const reveal = () => {
      if (list.clientWidth === 0) return
      const selected = list.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]')
      if (!selected) return
      const listBounds = list.getBoundingClientRect()
      const selectedBounds = selected.getBoundingClientRect()
      const margin = 12
      if (selectedBounds.left < listBounds.left + margin) list.scrollLeft -= listBounds.left + margin - selectedBounds.left
      else if (selectedBounds.right > listBounds.right - margin) list.scrollLeft += selectedBounds.right - listBounds.right + margin
    }
    reveal()
    if (typeof ResizeObserver === 'undefined') return
    const observer = new ResizeObserver(reveal)
    observer.observe(list)
    return () => observer.disconnect()
  }, [active])
  return listRef
}
