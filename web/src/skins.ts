// The skin axis: 15 themes taken from the ui-ux-pro-max style catalogue, plus
// Pangolin's own house look.
//
// A skin is expressed as a Mantine theme delta — geometry, elevation, type and
// per-component defaults — so the component library renders the theme rather
// than us hand-writing CSS for it. Only the handful of materials a component
// library cannot express (glass translucency, clay modelling, HUD glow, paper
// grain) get a small CSS layer, keyed off `data-material`.
//
// The catalogue warns that several of these styles are unsuitable for dense
// dashboards (neumorphism, editorial grid, aurora, claymorphism all say so, and
// skeuomorphism recommends flat instead). We take their visual language and
// hold the console's own contract: table text never drops below 14px, row
// height never compresses below 44px, and no expressive effect is allowed
// behind tabular data.

import type { Mode } from './palettes'

export type Material = 'flat' | 'glass' | 'aurora' | 'bevel' | 'soft' | 'clay' | 'block' | 'chrome' | 'hud' | 'paper'
export type Motion = 'crisp' | 'calm' | 'soft' | 'precise' | 'bouncy'
export type Density = 'comfortable' | 'compact'

export type Skin = {
  id: string
  /** Catalogue style this skin comes from; `house` is Pangolin's own. */
  basis: string
  material: Material
  motion: Motion
  density: Density
  /** `light`/`dark` when the style only works in one mode (e-ink, OLED). */
  preferredMode: Mode | 'auto'
  /** Palette that shows the skin as intended; the user can still change it. */
  recommendedPalette: string
  /** Mantine theme deltas. */
  radius: { xs: string; sm: string; md: string; lg: string; xl: string }
  shadows: { xs: string; sm: string; md: string; lg: string; xl: string }
  headings?: { fontFamily?: string; fontWeight?: string; letterSpacing?: string }
  /** Pushes text and borders past the standard floor (Accessible & Ethical). */
  highContrast?: boolean
  /** Mono headings/numerals are part of some identities (HUD, developer). */
  monoHeadings?: boolean
  monoNumerals?: boolean
  fontFamily?: string
}

const houseRadius = { xs: '6px', sm: '8px', md: '9px', lg: '14px', xl: '18px' }
const houseShadows = {
  xs: '0 1px 2px rgb(28 20 16 / .05)',
  sm: '0 1px 2px rgb(28 20 16 / .05), 0 10px 26px -18px rgb(28 20 16 / .18)',
  md: '0 1px 2px rgb(28 20 16 / .05), 0 14px 34px -16px rgb(28 20 16 / .16)',
  lg: '0 1px 3px rgb(28 20 16 / .07), 0 20px 50px -14px rgb(28 20 16 / .26)',
  xl: '0 1px 3px rgb(28 20 16 / .07), 0 28px 64px -16px rgb(28 20 16 / .3)',
}

const sans = "'Geist Variable', system-ui, -apple-system, 'Segoe UI', sans-serif"
const serif = "Georgia, 'Times New Roman', 'Songti SC', 'Noto Serif CJK SC', serif"

export const skins: Skin[] = [
  {
    id: 'house', basis: 'house', material: 'flat', motion: 'soft', density: 'comfortable', preferredMode: 'auto', recommendedPalette: 'bronze',
    radius: houseRadius, shadows: houseShadows, fontFamily: sans,
  },
  {
    // Catalogue: Minimalism & Swiss Style — grid, no ornament, type hierarchy only.
    id: 'swiss', basis: 'minimalism-and-swiss-style', material: 'flat', motion: 'crisp', density: 'comfortable', preferredMode: 'auto', recommendedPalette: 'banking-traditional',
    radius: { xs: '0px', sm: '2px', md: '3px', lg: '4px', xl: '6px' },
    shadows: { xs: 'none', sm: 'none', md: 'none', lg: 'none', xl: 'none' },
    headings: { fontWeight: '650', letterSpacing: '-0.03em' },
    fontFamily: sans,
  },
  {
    // Catalogue: E-Ink / Paper — paper stock, ink, grain, no fades.
    id: 'paper', basis: 'e-ink-paper', material: 'paper', motion: 'calm', density: 'comfortable', preferredMode: 'light', recommendedPalette: 'invoice-billing',
    radius: { xs: '2px', sm: '3px', md: '4px', lg: '6px', xl: '8px' },
    shadows: { xs: 'none', sm: 'none', md: 'none', lg: 'none', xl: 'none' },
    headings: { fontFamily: serif, fontWeight: '600', letterSpacing: '-0.01em' },
    fontFamily: sans,
  },
  {
    // Catalogue: Data-Dense Dashboard — built for exactly this product.
    id: 'dense', basis: 'data-dense-dashboard', material: 'flat', motion: 'crisp', density: 'compact', preferredMode: 'auto', recommendedPalette: 'developer-ide',
    radius: { xs: '4px', sm: '5px', md: '6px', lg: '8px', xl: '10px' },
    shadows: { xs: 'none', sm: '0 1px 2px rgb(0 0 0 / .06)', md: '0 1px 3px rgb(0 0 0 / .08)', lg: '0 4px 14px -6px rgb(0 0 0 / .16)', xl: '0 8px 24px -8px rgb(0 0 0 / .2)' },
    headings: { fontWeight: '600', letterSpacing: '-0.014em' },
    fontFamily: sans,
  },
  {
    // Catalogue: Accessible & Ethical — 7:1 text, 3px focus ring, 44px targets.
    id: 'accessible', basis: 'accessible-and-ethical', material: 'flat', motion: 'calm', density: 'comfortable', preferredMode: 'auto', recommendedPalette: 'banking-traditional', highContrast: true,
    radius: { xs: '6px', sm: '8px', md: '10px', lg: '12px', xl: '14px' },
    shadows: { xs: 'none', sm: 'none', md: 'none', lg: '0 1px 3px rgb(0 0 0 / .12)', xl: '0 4px 12px -4px rgb(0 0 0 / .18)' },
    headings: { fontWeight: '650' },
    fontFamily: sans,
  },
  {
    // Catalogue: Fluent 2 — calm depth, platform-adaptive motion.
    id: 'fluent', basis: 'fluent-2', material: 'soft', motion: 'soft', density: 'comfortable', preferredMode: 'auto', recommendedPalette: 'saas-general',
    radius: { xs: '3px', sm: '4px', md: '6px', lg: '8px', xl: '12px' },
    shadows: { xs: '0 1px 2px rgb(0 0 0 / .05)', sm: '0 1px 2px rgb(0 0 0 / .07), 0 2px 6px -2px rgb(0 0 0 / .08)', md: '0 2px 4px rgb(0 0 0 / .07), 0 8px 16px -8px rgb(0 0 0 / .12)', lg: '0 4px 8px rgb(0 0 0 / .08), 0 16px 32px -12px rgb(0 0 0 / .16)', xl: '0 8px 16px rgb(0 0 0 / .1), 0 24px 48px -16px rgb(0 0 0 / .2)' },
    headings: { fontWeight: '600', letterSpacing: '-0.01em' },
    fontFamily: sans,
  },
  {
    // Catalogue: Glassmorphism — translucent chrome over an ambient field.
    id: 'glass', basis: 'glassmorphism', material: 'glass', motion: 'soft', density: 'comfortable', preferredMode: 'light', recommendedPalette: 'biotech',
    radius: { xs: '8px', sm: '12px', md: '14px', lg: '18px', xl: '22px' },
    shadows: { xs: '0 1px 2px rgb(15 23 42 / .04)', sm: '0 8px 24px -16px rgb(15 23 42 / .18)', md: '0 12px 32px -18px rgb(15 23 42 / .2)', lg: '0 20px 48px -20px rgb(15 23 42 / .26)', xl: '0 28px 64px -24px rgb(15 23 42 / .3)' },
    headings: { fontWeight: '620', letterSpacing: '-0.02em' },
    fontFamily: sans,
  },
  {
    // Catalogue: Aurora UI — flowing gradient field, colour layering as depth.
    id: 'aurora', basis: 'aurora-ui', material: 'aurora', motion: 'soft', density: 'comfortable', preferredMode: 'dark', recommendedPalette: 'ai-chatbot',
    radius: { xs: '8px', sm: '12px', md: '16px', lg: '20px', xl: '24px' },
    shadows: { xs: '0 1px 2px rgb(0 0 0 / .2)', sm: '0 8px 24px -18px rgb(0 0 0 / .5)', md: '0 12px 36px -20px rgb(0 0 0 / .55)', lg: '0 20px 52px -22px rgb(0 0 0 / .6)', xl: '0 28px 72px -26px rgb(0 0 0 / .65)' },
    headings: { fontWeight: '640', letterSpacing: '-0.024em' },
    fontFamily: sans,
  },
  {
    // Catalogue: Skeuomorphism — real materials, multi-layer light and shadow.
    id: 'skeuo', basis: 'skeuomorphism', material: 'bevel', motion: 'soft', density: 'comfortable', preferredMode: 'light', recommendedPalette: 'bronze',
    radius: { xs: '5px', sm: '7px', md: '10px', lg: '14px', xl: '18px' },
    shadows: { xs: '0 1px 1px rgb(60 40 25 / .12)', sm: 'inset 0 1px 0 rgb(255 255 255 / .7), 0 1px 1px rgb(60 40 25 / .14), 0 2px 4px rgb(60 40 25 / .1)', md: 'inset 0 1px 0 rgb(255 255 255 / .75), 0 1px 2px rgb(60 40 25 / .14), 0 6px 12px -6px rgb(60 40 25 / .22)', lg: 'inset 0 1px 0 rgb(255 255 255 / .8), 0 2px 3px rgb(60 40 25 / .16), 0 16px 32px -14px rgb(60 40 25 / .3)', xl: 'inset 0 1px 0 rgb(255 255 255 / .8), 0 3px 5px rgb(60 40 25 / .18), 0 24px 48px -18px rgb(60 40 25 / .34)' },
    headings: { fontWeight: '680', letterSpacing: '-0.016em' },
    fontFamily: sans,
  },
  {
    // Catalogue: Neumorphism — one light source, dual soft shadows. The catalogue
    // flags this as high accessibility risk, so borders and focus stay explicit.
    id: 'neumorph', basis: 'neumorphism', material: 'soft', motion: 'soft', density: 'comfortable', preferredMode: 'light', recommendedPalette: 'productivity-tool',
    radius: { xs: '10px', sm: '12px', md: '14px', lg: '16px', xl: '20px' },
    shadows: { xs: '-2px -2px 6px rgb(255 255 255 / .7), 2px 2px 6px rgb(0 0 0 / .07)', sm: '-4px -4px 10px rgb(255 255 255 / .75), 5px 5px 12px rgb(0 0 0 / .09)', md: '-5px -5px 15px rgb(255 255 255 / .8), 6px 6px 16px rgb(0 0 0 / .1)', lg: '-8px -8px 22px rgb(255 255 255 / .85), 10px 10px 26px rgb(0 0 0 / .12)', xl: '-10px -10px 28px rgb(255 255 255 / .85), 14px 14px 34px rgb(0 0 0 / .14)' },
    headings: { fontWeight: '600', letterSpacing: '-0.012em' },
    fontFamily: sans,
  },
  {
    // Catalogue: Claymorphism — modelled clay, thick soft edges, gentle bounce.
    id: 'clay', basis: 'claymorphism', material: 'clay', motion: 'bouncy', density: 'comfortable', preferredMode: 'light', recommendedPalette: 'pet-tech',
    radius: { xs: '14px', sm: '16px', md: '20px', lg: '24px', xl: '28px' },
    shadows: { xs: 'inset -2px -2px 5px rgb(0 0 0 / .05), 3px 3px 6px rgb(0 0 0 / .07)', sm: 'inset -2px -2px 6px rgb(0 0 0 / .06), 5px 5px 10px rgb(0 0 0 / .08)', md: 'inset -3px -3px 8px rgb(0 0 0 / .06), 6px 6px 14px rgb(0 0 0 / .1)', lg: 'inset -4px -4px 10px rgb(0 0 0 / .07), 10px 10px 22px rgb(0 0 0 / .12)', xl: 'inset -5px -5px 12px rgb(0 0 0 / .08), 14px 14px 30px rgb(0 0 0 / .14)' },
    headings: { fontWeight: '700', letterSpacing: '-0.018em' },
    fontFamily: sans,
  },
  {
    // Catalogue: Vibrant & Block-based — colour blocks, hard edges, loud type.
    id: 'vibrant', basis: 'vibrant-and-block-based', material: 'block', motion: 'crisp', density: 'comfortable', preferredMode: 'light', recommendedPalette: 'gaming',
    radius: { xs: '4px', sm: '6px', md: '8px', lg: '12px', xl: '16px' },
    shadows: { xs: '2px 2px 0 rgb(0 0 0 / .85)', sm: '3px 3px 0 rgb(0 0 0 / .85)', md: '4px 4px 0 rgb(0 0 0 / .85)', lg: '6px 6px 0 rgb(0 0 0 / .85)', xl: '8px 8px 0 rgb(0 0 0 / .85)' },
    headings: { fontWeight: '800', letterSpacing: '-0.03em' },
    fontFamily: sans,
  },
  {
    // Catalogue: Y2K Aesthetic — chrome gradients and gloss.
    id: 'y2k', basis: 'y2k-aesthetic', material: 'chrome', motion: 'crisp', density: 'comfortable', preferredMode: 'auto', recommendedPalette: 'space-tech',
    radius: { xs: '8px', sm: '10px', md: '14px', lg: '18px', xl: '22px' },
    shadows: { xs: '0 1px 0 rgb(255 255 255 / .6) inset', sm: 'inset 0 1px 0 rgb(255 255 255 / .7), 0 2px 6px rgb(15 23 42 / .12)', md: 'inset 0 1px 0 rgb(255 255 255 / .75), 0 6px 18px -8px rgb(15 23 42 / .28)', lg: 'inset 0 1px 0 rgb(255 255 255 / .8), 0 14px 34px -14px rgb(15 23 42 / .34)', xl: 'inset 0 1px 0 rgb(255 255 255 / .8), 0 20px 48px -18px rgb(15 23 42 / .38)' },
    headings: { fontWeight: '700', letterSpacing: '-0.02em' },
    fontFamily: sans,
  },
  {
    // Catalogue: HUD / Sci-Fi FUI — hairline grid, glow, monospace. Flagged high
    // risk, so the glow stays on chrome and never behind data.
    id: 'hud', basis: 'hud-sci-fi-fui', material: 'hud', motion: 'precise', density: 'compact', preferredMode: 'dark', recommendedPalette: 'cybersecurity',
    radius: { xs: '0px', sm: '1px', md: '2px', lg: '3px', xl: '4px' },
    shadows: { xs: 'none', sm: '0 0 4px rgb(0 0 0 / .5)', md: '0 0 8px rgb(0 0 0 / .5)', lg: '0 0 18px rgb(0 0 0 / .55)', xl: '0 0 28px rgb(0 0 0 / .6)' },
    headings: { fontWeight: '600', letterSpacing: '0.04em' },
    monoHeadings: true, monoNumerals: true,
    fontFamily: sans,
  },
  {
    // Catalogue: Dark Mode (OLED) — true black, minimal glow, low emission.
    id: 'oled', basis: 'dark-mode-oled', material: 'flat', motion: 'crisp', density: 'comfortable', preferredMode: 'dark', recommendedPalette: 'api-portal',
    radius: { xs: '4px', sm: '6px', md: '8px', lg: '10px', xl: '12px' },
    shadows: { xs: 'none', sm: 'none', md: '0 0 0 1px rgb(255 255 255 / .06)', lg: '0 0 24px -6px rgb(0 0 0 / .8)', xl: '0 0 40px -8px rgb(0 0 0 / .85)' },
    headings: { fontWeight: '620', letterSpacing: '-0.02em' },
    fontFamily: sans,
  },
  {
    // Catalogue: Editorial Grid / Magazine — print hierarchy, serif headings.
    id: 'editorial', basis: 'editorial-grid-magazine', material: 'paper', motion: 'calm', density: 'comfortable', preferredMode: 'light', recommendedPalette: 'invoice-billing',
    radius: { xs: '0px', sm: '0px', md: '0px', lg: '2px', xl: '2px' },
    shadows: { xs: 'none', sm: 'none', md: 'none', lg: 'none', xl: 'none' },
    headings: { fontFamily: serif, fontWeight: '700', letterSpacing: '-0.02em' },
    fontFamily: sans,
  },
]

export const skinById = new Map(skins.map((skin) => [skin.id, skin]))

export function resolveSkin(id: string): Skin {
  return skinById.get(id) ?? skins[0]
}

/** The catalogue styles that only work in one mode force it when selected. */
export function modeForSkin(skin: Skin, current: Mode): Mode {
  return skin.preferredMode === 'auto' ? current : skin.preferredMode
}

// Labels live beside the skin data rather than in the i18n bundle: they name a
// design language, and a translator cannot improve "Neumorphism".
const labels: Record<string, { en: string; zh: string }> = {
  house: { en: 'Pangolin house', zh: '鲮鲤默认' },
  swiss: { en: 'Swiss minimal', zh: '瑞士极简' },
  paper: { en: 'E-ink paper', zh: '电子墨水纸' },
  dense: { en: 'Data-dense', zh: '数据密集' },
  accessible: { en: 'High contrast', zh: '高对比无障碍' },
  fluent: { en: 'Fluent', zh: 'Fluent 企业风' },
  glass: { en: 'Frosted glass', zh: '磨砂玻璃' },
  aurora: { en: 'Aurora', zh: '极光渐变' },
  skeuo: { en: 'Skeuomorphic', zh: '拟物' },
  neumorph: { en: 'Soft UI', zh: '软 UI' },
  clay: { en: 'Clay', zh: '黏土圆润' },
  vibrant: { en: 'Vibrant blocks', zh: '高饱和色块' },
  y2k: { en: 'Y2K chrome', zh: 'Y2K 铬金属' },
  hud: { en: 'Sci-fi HUD', zh: '科幻 HUD' },
  oled: { en: 'OLED black', zh: '纯黑 OLED' },
  editorial: { en: 'Editorial', zh: '杂志编辑' },
}

export function skinLabel(id: string, language: string): string {
  const label = labels[id] ?? labels.house
  return language.startsWith('zh') ? label.zh : label.en
}
