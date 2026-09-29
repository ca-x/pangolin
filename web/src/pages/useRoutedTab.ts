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
