/** Apply one model or a developer group without silently importing only part of it. */
export function nextModelImportSelection(current: string[], ids: string[], checked: boolean, maximum: number): string[] | null {
  const next = new Set(current)
  for (const id of ids) checked ? next.add(id) : next.delete(id)
  return next.size > maximum ? null : [...next]
}
