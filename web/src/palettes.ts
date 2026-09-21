// The palette axis: 20 colour families, each resolving to a full semantic token
// set for both light and dark.
//
// Provenance: 17 palettes are taken verbatim from the ui-ux-pro-max colour
// catalogue (192 product palettes) — the row supplies primary/background/card/
// foreground/muted/border/destructive/ring for the one mode that catalogue row
// describes. The catalogue gives each product a single mode, so the counterpart
// mode is derived here: hue preserved, lightness re-anchored in OKLCH, chroma
// clamped into sRGB. `web/src/palettes.test.ts` audits every combination with
// WCAG maths rather than trusting the derivation.
//
// The remaining 3 are Pangolin's own brand palettes (bronze/slate/jade), which
// keep both modes verbatim so the product's existing look stays the default.

import { contrastRatio, ensureContrast, oklchToRgb, parseHex, readableInk, rgbToOklch, toHex, withLightness, type Oklch } from './color'

export type Mode = 'light' | 'dark'

export type PaletteTokens = {
  canvas: string
  surface: string
  surfaceMuted: string
  elevated: string
  ink: string
  mutedInk: string
  faintInk: string
  border: string
  borderSubtle: string
  controlBorder: string
  accent: string
  accentStrong: string
  accentSoft: string
  accentContrast: string
  success: string
  successSoft: string
  warning: string
  warningSoft: string
  danger: string
  dangerSoft: string
  focus: string
}

type CatalogRow = {
  id: string
  product: string
  labelZh: string
  mode: Mode
  primary: string
  onPrimary: string
  background: string
  card: string
  foreground: string
  muted: string
  mutedForeground: string
  border: string
  destructive: string
  ring: string
}

export type Palette = {
  id: string
  /** Catalogue product this palette comes from, shown in the settings hint. */
  basis: string
  labelEn: string
  labelZh: string
  brand?: boolean
  light: PaletteTokens
  dark: PaletteTokens
}

const catalog: readonly CatalogRow[] = [
  { id: 'saas-general', product: 'SaaS (General)', labelZh: '通用 SaaS', mode: 'light', primary: '#2563EB', onPrimary: '#FFFFFF', background: '#F8FAFC', card: '#FFFFFF', foreground: '#1E293B', muted: '#E9EFF8', mutedForeground: '#475569', border: '#E2E8F0', destructive: '#DC2626', ring: '#2563EB' },
  { id: 'analytics-dashboard', product: 'Analytics Dashboard', labelZh: '分析看板', mode: 'light', primary: '#1E40AF', onPrimary: '#FFFFFF', background: '#F8FAFC', card: '#FFFFFF', foreground: '#1E3A8A', muted: '#E9EEF6', mutedForeground: '#475569', border: '#DBEAFE', destructive: '#DC2626', ring: '#1E40AF' },
  { id: 'invoice-billing', product: 'Invoice & Billing Tool', labelZh: '账单与发票', mode: 'light', primary: '#1E3A5F', onPrimary: '#FFFFFF', background: '#F8FAFC', card: '#FFFFFF', foreground: '#0F172A', muted: '#F1F3F5', mutedForeground: '#475569', border: '#E4E7EB', destructive: '#DC2626', ring: '#1E3A5F' },
  { id: 'banking-traditional', product: 'Banking/Traditional Finance', labelZh: '传统银行', mode: 'light', primary: '#0F172A', onPrimary: '#FFFFFF', background: '#F8FAFC', card: '#FFFFFF', foreground: '#020617', muted: '#E8ECF1', mutedForeground: '#475569', border: '#E2E8F0', destructive: '#DC2626', ring: '#0F172A' },
  { id: 'healthcare', product: 'Healthcare App', labelZh: '医疗健康', mode: 'light', primary: '#0891B2', onPrimary: '#000000', background: '#ECFEFF', card: '#FFFFFF', foreground: '#164E63', muted: '#E8F1F6', mutedForeground: '#475569', border: '#A5F3FC', destructive: '#DC2626', ring: '#0891B2' },
  { id: 'biotech', product: 'Biotech / Life Sciences', labelZh: '生物科技', mode: 'light', primary: '#0EA5E9', onPrimary: '#0F172A', background: '#F0F9FF', card: '#FFFFFF', foreground: '#0C4A6E', muted: '#E8F2F8', mutedForeground: '#475569', border: '#BAE6FD', destructive: '#DC2626', ring: '#0EA5E9' },
  { id: 'climate-tech', product: 'Sustainable Energy / Climate Tech', labelZh: '可持续能源', mode: 'light', primary: '#059669', onPrimary: '#000000', background: '#ECFDF5', card: '#FFFFFF', foreground: '#064E3B', muted: '#E8F1F3', mutedForeground: '#475569', border: '#A7F3D0', destructive: '#DC2626', ring: '#059669' },
  { id: 'ai-chatbot', product: 'AI/Chatbot Platform', labelZh: 'AI 对话平台', mode: 'light', primary: '#7C3AED', onPrimary: '#FFFFFF', background: '#FAF5FF', card: '#FFFFFF', foreground: '#1E1B4B', muted: '#ECEEF9', mutedForeground: '#475569', border: '#DDD6FE', destructive: '#DC2626', ring: '#7C3AED' },
  { id: 'pet-tech', product: 'Pet Tech App', labelZh: '宠物科技', mode: 'light', primary: '#F97316', onPrimary: '#0F172A', background: '#FFF7ED', card: '#FFFFFF', foreground: '#9A3412', muted: '#F1F0F0', mutedForeground: '#475569', border: '#FED7AA', destructive: '#DC2626', ring: '#F97316' },
  { id: 'productivity-tool', product: 'Productivity Tool', labelZh: '效率工具', mode: 'light', primary: '#0D9488', onPrimary: '#000000', background: '#F0FDFA', card: '#FFFFFF', foreground: '#134E4A', muted: '#E8F1F4', mutedForeground: '#475569', border: '#99F6E4', destructive: '#DC2626', ring: '#0D9488' },
  { id: 'developer-ide', product: 'Developer Tool / IDE', labelZh: '开发者工具', mode: 'dark', primary: '#1E293B', onPrimary: '#FFFFFF', background: '#0F172A', card: '#1B2336', foreground: '#F8FAFC', muted: '#272F42', mutedForeground: '#94A3B8', border: '#475569', destructive: '#EF4444', ring: '#FFFFFF' },
  { id: 'api-portal', product: 'API Developer Portal', labelZh: 'API 门户', mode: 'dark', primary: '#0F172A', onPrimary: '#FFFFFF', background: '#020617', card: '#0E1223', foreground: '#F8FAFC', muted: '#1A1E2F', mutedForeground: '#94A3B8', border: '#334155', destructive: '#EF4444', ring: '#FFFFFF' },
  { id: 'fintech-crypto', product: 'Fintech/Crypto', labelZh: '金融科技', mode: 'dark', primary: '#F59E0B', onPrimary: '#0F172A', background: '#0F172A', card: '#222735', foreground: '#F8FAFC', muted: '#272F42', mutedForeground: '#94A3B8', border: '#334155', destructive: '#EF4444', ring: '#F59E0B' },
  { id: 'cybersecurity', product: 'Cybersecurity Platform', labelZh: '网络安全', mode: 'dark', primary: '#00FF41', onPrimary: '#0F172A', background: '#000000', card: '#0C130E', foreground: '#E0E0E0', muted: '#181818', mutedForeground: '#94A3B8', border: '#1F1F1F', destructive: '#EF4444', ring: '#00FF41' },
  { id: 'space-tech', product: 'Space Tech / Aerospace', labelZh: '航天科技', mode: 'dark', primary: '#F8FAFC', onPrimary: '#0F172A', background: '#0B0B10', card: '#1E1E23', foreground: '#F8FAFC', muted: '#232328', mutedForeground: '#94A3B8', border: '#1E293B', destructive: '#EF4444', ring: '#F8FAFC' },
  { id: 'calculator', product: 'Calculator & Unit Converter', labelZh: '计算器', mode: 'dark', primary: '#EA580C', onPrimary: '#000000', background: '#1C1917', card: '#262321', foreground: '#FFFFFF', muted: '#2C1E16', mutedForeground: '#94A3B8', border: 'rgba(255,255,255,0.08)', destructive: '#DC2626', ring: '#EA580C' },
  { id: 'gaming', product: 'Gaming', labelZh: '游戏', mode: 'dark', primary: '#7C3AED', onPrimary: '#FFFFFF', background: '#0F0F23', card: '#1E1C35', foreground: '#E2E8F0', muted: '#27273B', mutedForeground: '#94A3B8', border: '#4C1D95', destructive: '#EF4444', ring: '#7C3AED' },
]

/** Composites `rgba()`/`rgb()` onto a backdrop so every token stays a hex. */
function normalize(value: string, backdrop: string): string {
  const match = value.match(/rgba?\(([^)]+)\)/i)
  if (!match) return value
  const parts = match[1].split(',').map((part) => Number.parseFloat(part.trim()))
  const [r, g, b] = parts
  const alpha = parts.length > 3 ? parts[3] : 1
  const base = parseHex(backdrop)
  return toHex({
    r: (r / 255) * alpha + base.r * (1 - alpha),
    g: (g / 255) * alpha + base.g * (1 - alpha),
    b: (b / 255) * alpha + base.b * (1 - alpha),
  })
}

const step = (l: number, c: number, h: number): string => toHex(oklchToRgb({ l, c, h }))

/** Darkens a colour to an OKLCH lightness, keeping its hue. */
const atLightness = (hex: string, lightness: number, chromaCap = 0.14) => withLightness(hex, lightness, chromaCap)

const successLight = '#2c6f57'
const warningLight = '#96631a'

/**
 * Enforces the contrast contract on a resolved token set: 4.5:1 for anything
 * that carries text (including status colours on their own soft fills), 3:1 for
 * the boundary that identifies a control, and 3:1 for the faintest micro label.
 * Colours that already pass are left exactly as authored — the guard only moves
 * the ones that fail, and only as far as the target requires.
 */
function guard(tokens: PaletteTokens): PaletteTokens {
  const against = (value: string, background: string, target: number) => ensureContrast(value, background, target)
  const onCanvasAndSurface = (value: string, target: number) =>
    against(against(value, tokens.canvas, target), tokens.surface, target)
  const guarded: PaletteTokens = {
    ...tokens,
    ink: onCanvasAndSurface(tokens.ink, 4.5),
    mutedInk: onCanvasAndSurface(tokens.mutedInk, 4.5),
    faintInk: onCanvasAndSurface(tokens.faintInk, 3),
    controlBorder: onCanvasAndSurface(tokens.controlBorder, 3),
    border: against(tokens.border, tokens.surface, 1.15),
    accent: against(tokens.accent, tokens.canvas, 3),
    accentStrong: against(tokens.accentStrong, tokens.accentSoft, 4.5),
    success: against(tokens.success, tokens.successSoft, 4.5),
    warning: against(tokens.warning, tokens.warningSoft, 4.5),
    danger: against(tokens.danger, tokens.dangerSoft, 4.5),
  }
  guarded.danger = onCanvasAndSurface(guarded.danger, 4.5)
  guarded.success = against(guarded.success, tokens.canvas, 4.5)
  guarded.warning = against(guarded.warning, tokens.canvas, 4.5)
  const lower = (value: string) => value.toLowerCase()
  return Object.fromEntries(Object.entries(guarded).map(([token, value]) => [token, lower(value)])) as PaletteTokens
}

function fromCatalog(row: CatalogRow, mode: Mode): PaletteTokens {
  return guard(rawFromCatalog(row, mode))
}

function rawFromCatalog(row: CatalogRow, mode: Mode): PaletteTokens {
  const source: Mode = row.mode
  const hue: number = rgbToOklch(parseHex(row.primary)).h
  const canvasHue: number = rgbToOklch(parseHex(row.background)).h
  const accent = atLightness(row.primary, source === 'light' ? rgbToOklch(parseHex(row.primary)).l : Math.max(0.62, rgbToOklch(parseHex(row.primary)).l))

  if (mode === source) {
    // Verbatim: the catalogue row already describes this mode.
    const ink = row.foreground
    const surface = normalize(row.card, row.background)
    const canvas = normalize(row.background, '#ffffff')
    const accentSource = row.mode === 'dark' ? accent : row.primary
    return {
      canvas,
      surface,
      surfaceMuted: normalize(row.muted, surface),
      elevated: surface,
      ink,
      mutedInk: row.mutedForeground,
      faintInk: atLightness(row.mutedForeground, row.mode === 'light' ? Math.min(0.62, rgbToOklch(parseHex(row.mutedForeground)).l + 0.1) : Math.max(0.66, rgbToOklch(parseHex(row.mutedForeground)).l)),
      border: normalize(row.border, surface),
      borderSubtle: atLightness(normalize(row.border, surface), rgbToOklch(parseHex(normalize(row.border, surface))).l + (row.mode === 'light' ? 0.04 : 0.05), 0.02),
      controlBorder: atLightness(normalize(row.border, surface), rgbToOklch(parseHex(normalize(row.border, surface))).l - (row.mode === 'light' ? 0.12 : 0.1), 0.03),
      accent: accentSource,
      accentStrong: atLightness(accentSource, row.mode === 'light' ? rgbToOklch(parseHex(accentSource)).l - 0.09 : Math.min(0.92, rgbToOklch(parseHex(accentSource)).l + 0.1)),
      accentSoft: row.mode === 'light'
        ? step(0.94, Math.min(0.05, rgbToOklch(parseHex(accentSource)).c * 0.4), hue)
        : step(0.27, Math.min(0.06, rgbToOklch(parseHex(accentSource)).c * 0.4), hue),
      accentContrast: contrastRatio(row.onPrimary, accentSource) >= 4.5 ? row.onPrimary : readableInk(accentSource),
      success: row.mode === 'light' ? successLight : atLightness(successLight, 0.74, 0.09),
      successSoft: row.mode === 'light' ? step(0.94, 0.035, rgbToOklch(parseHex(successLight)).h) : step(0.26, 0.045, rgbToOklch(parseHex(successLight)).h),
      warning: row.mode === 'light' ? warningLight : atLightness(warningLight, 0.78, 0.1),
      warningSoft: row.mode === 'light' ? step(0.94, 0.04, rgbToOklch(parseHex(warningLight)).h) : step(0.28, 0.05, rgbToOklch(parseHex(warningLight)).h),
      danger: row.mode === 'light' ? row.destructive : atLightness(row.destructive, 0.7, 0.13),
      dangerSoft: row.mode === 'light' ? step(0.95, 0.04, rgbToOklch(parseHex(row.destructive)).h) : step(0.28, 0.06, rgbToOklch(parseHex(row.destructive)).h),
      focus: normalize(row.ring, row.background) === '#000000' || normalize(row.ring, row.background) === '#FFFFFF'
        ? accentSource
        : normalize(row.ring, row.background),
    }
  }

  // Counterpart mode: same hue, re-anchored lightness. A derived dark mode uses
  // near-black neutrals carrying a trace of the accent hue so it does not read
  // as a different product.
  if (mode === 'dark') {
    const derivedAccent = atLightness(row.primary, 0.72, Math.min(0.13, rgbToOklch(parseHex(row.primary)).c))
    return {
      canvas: step(0.16, 0.012, canvasHue),
      surface: step(0.21, 0.014, canvasHue),
      surfaceMuted: step(0.25, 0.014, canvasHue),
      elevated: step(0.27, 0.014, canvasHue),
      ink: step(0.96, 0.006, canvasHue),
      mutedInk: step(0.78, 0.012, canvasHue),
      faintInk: step(0.68, 0.012, canvasHue),
      border: step(0.32, 0.014, canvasHue),
      borderSubtle: step(0.27, 0.012, canvasHue),
      controlBorder: step(0.46, 0.016, canvasHue),
      accent: derivedAccent,
      accentStrong: atLightness(row.primary, 0.84, Math.min(0.13, rgbToOklch(parseHex(row.primary)).c)),
      accentSoft: step(0.28, Math.min(0.06, rgbToOklch(parseHex(row.primary)).c * 0.4), hue),
      accentContrast: readableInk(derivedAccent),
      success: atLightness(successLight, 0.74, 0.09),
      successSoft: step(0.26, 0.045, rgbToOklch(parseHex(successLight)).h),
      warning: atLightness(warningLight, 0.78, 0.1),
      warningSoft: step(0.28, 0.05, rgbToOklch(parseHex(warningLight)).h),
      danger: atLightness(row.destructive, 0.7, 0.13),
      dangerSoft: step(0.28, 0.06, rgbToOklch(parseHex(row.destructive)).h),
      focus: derivedAccent,
    }
  }

  const sourceAccentL = rgbToOklch(parseHex(row.primary)).l
  const derivedAccent = sourceAccentL <= 0.45 ? row.primary : atLightness(row.primary, 0.5, Math.min(0.14, rgbToOklch(parseHex(row.primary)).c))
  return {
    canvas: step(0.975, Math.min(0.012, rgbToOklch(parseHex(row.background)).c), canvasHue),
    surface: step(0.998, 0.003, canvasHue),
    surfaceMuted: step(0.965, 0.008, canvasHue),
    elevated: step(1, 0, canvasHue),
    ink: step(0.22, 0.015, canvasHue),
    mutedInk: step(0.48, 0.014, canvasHue),
    faintInk: step(0.58, 0.014, canvasHue),
    border: step(0.9, 0.008, canvasHue),
    borderSubtle: step(0.94, 0.006, canvasHue),
    controlBorder: step(0.78, 0.012, canvasHue),
    accent: derivedAccent,
    accentStrong: atLightness(derivedAccent, Math.max(0.3, rgbToOklch(parseHex(derivedAccent)).l - 0.1), 0.14),
    accentSoft: step(0.94, Math.min(0.05, rgbToOklch(parseHex(row.primary)).c * 0.4), hue),
    accentContrast: readableInk(derivedAccent),
    success: successLight,
    successSoft: step(0.94, 0.035, rgbToOklch(parseHex(successLight)).h),
    warning: warningLight,
    warningSoft: step(0.94, 0.04, rgbToOklch(parseHex(warningLight)).h),
    danger: atLightness(row.destructive, 0.5, 0.16),
    dangerSoft: step(0.95, 0.04, rgbToOklch(parseHex(row.destructive)).h),
    focus: derivedAccent,
  }
}

/** Pangolin's own three, with both modes authored rather than derived. */
function brand(id: string, basis: string, labelEn: string, labelZh: string, light: PaletteTokens, dark: PaletteTokens): Palette {
  return { id, basis, labelEn, labelZh, brand: true, light: guard(light), dark: guard(dark) }
}

const bronzeLight: PaletteTokens = {
  canvas: '#f2efe9', surface: '#ffffff', surfaceMuted: '#f7f4ef', elevated: '#ffffff',
  ink: '#1c1917', mutedInk: '#6b635c', faintInk: '#8e857d',
  border: '#e6e1d8', borderSubtle: '#efeae1', controlBorder: '#c9c0b6',
  accent: '#9a5a38', accentStrong: '#7b4227', accentSoft: '#f5eae1', accentContrast: '#ffffff',
  success: '#2c6f57', successSoft: '#e2efe8', warning: '#96631a', warningSoft: '#f7ecd7',
  danger: '#b23a34', dangerSoft: '#f9e6e3', focus: '#7a5872',
}
const bronzeDark: PaletteTokens = {
  canvas: '#131110', surface: '#1c1917', surfaceMuted: '#221e1c', elevated: '#272220',
  ink: '#f3eeea', mutedInk: '#a79e98', faintInk: '#94897f',
  border: '#3a3431', borderSubtle: '#2f2a27', controlBorder: '#574e4a',
  accent: '#d78f6b', accentStrong: '#eaa87f', accentSoft: '#3a2620', accentContrast: '#1b1210',
  success: '#6cb091', successSoft: '#1e332b', warning: '#d2a154', warningSoft: '#3a2e18',
  danger: '#e57f78', dangerSoft: '#3d2320', focus: '#bb96b2',
}

const slateLight: PaletteTokens = {
  ...bronzeLight, accent: '#4f6f8f', accentStrong: '#345675', accentSoft: '#e6edf3', accentContrast: '#ffffff', focus: '#4f6f8f',
}
const slateDark: PaletteTokens = {
  ...bronzeDark, accent: '#83a8ca', accentStrong: '#a4c4df', accentSoft: '#20303e', accentContrast: '#101a24', focus: '#83a8ca',
}
const jadeLight: PaletteTokens = {
  ...bronzeLight, accent: '#2f6f5e', accentStrong: '#1d5544', accentSoft: '#e0ede8', accentContrast: '#ffffff', focus: '#2f6f5e',
}
const jadeDark: PaletteTokens = {
  ...bronzeDark, accent: '#78b5a1', accentStrong: '#9bd0bd', accentSoft: '#1c312b', accentContrast: '#0d1a16', focus: '#78b5a1',
}

export const palettes: Palette[] = [
  brand('bronze', 'Pangolin brand', 'Bronze', '铜棕', bronzeLight, bronzeDark),
  brand('slate', 'Pangolin brand', 'Slate', '石板蓝', slateLight, slateDark),
  brand('jade', 'Pangolin brand', 'Jade', '玉青', jadeLight, jadeDark),
  ...catalog.map((row) => ({
    id: row.id,
    basis: row.product,
    labelEn: row.product,
    labelZh: row.labelZh,
    light: fromCatalog(row, 'light'),
    dark: fromCatalog(row, 'dark'),
  })),
]

export const paletteById = new Map(palettes.map((palette) => [palette.id, palette]))

export function resolvePalette(id: string, mode: Mode): PaletteTokens {
  const palette = paletteById.get(id) ?? palettes[0]
  return palette[mode]
}

/** Token name → CSS custom property. Shared by the app and the audit test. */
export const tokenToVariable: Record<keyof PaletteTokens, string> = {
  canvas: '--canvas',
  surface: '--surface',
  surfaceMuted: '--surface-muted',
  elevated: '--elevated',
  ink: '--ink',
  mutedInk: '--muted',
  faintInk: '--faint',
  border: '--border',
  borderSubtle: '--border-subtle',
  controlBorder: '--control-border',
  accent: '--accent',
  accentStrong: '--accent-strong',
  accentSoft: '--accent-soft',
  accentContrast: '--accent-contrast',
  success: '--success',
  successSoft: '--success-soft',
  warning: '--warning',
  warningSoft: '--warning-soft',
  danger: '--danger',
  dangerSoft: '--danger-soft',
  focus: '--focus',
}

export type { Oklch }
