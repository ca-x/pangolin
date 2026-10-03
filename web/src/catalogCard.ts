/** Safe facts from the immutable admin catalog snapshot, never raw extensions. */
export const CARD_CAPABILITIES = [
  'streaming',
  'tools',
  'reasoning',
  'temperature',
  'vision',
  'json_schema',
  'web_search',
  'file_search',
  'computer_use',
  'caching',
  'batch',
] as const
export type CatalogCard = {
  capabilities: Record<(typeof CARD_CAPABILITIES)[number], boolean | null>
  limits: { context: number | null; output: number | null }
  modalities: { input: string[] | null; output: string[] | null }
  reasoning_levels: string[] | null
}
function object(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}
function list(value: unknown): string[] | null {
  if (
    !Array.isArray(value) ||
    value.length > 16 ||
    !value.every(
      (item) =>
        typeof item === 'string' &&
        item.length > 0 &&
        new TextEncoder().encode(item).byteLength <= 32 &&
        !/[\u0000-\u001f\u007f-\u009f]/.test(item),
    )
  )
    return null
  return [...new Set(value as string[])]
}
function limit(value: unknown): number | null {
  return typeof value === 'number' && Number.isSafeInteger(value) && value > 0 ? value : null
}
export function catalogCard(metadata: unknown): CatalogCard | null {
  if (!object(metadata) || !object(metadata.card)) return null
  const card = metadata.card
  const capabilities = object(card.capabilities) ? card.capabilities : {}
  const limits = object(card.limits) ? card.limits : {}
  const modalities = object(card.modalities) ? card.modalities : {}
  return {
    capabilities: Object.fromEntries(
      CARD_CAPABILITIES.map((key) => [key, typeof capabilities[key] === 'boolean' ? capabilities[key] : null]),
    ) as CatalogCard['capabilities'],
    limits: { context: limit(limits.context), output: limit(limits.output) },
    modalities: { input: list(modalities.input), output: list(modalities.output) },
    reasoning_levels: list(card.reasoning_levels),
  }
}
