// The theme layer. Everything the console renders — buttons, inputs, tables,
// modals, tooltips, badges — comes from Mantine; this file decides what Mantine
// looks like. A skin supplies geometry, elevation and typography, a palette
// supplies colour, and the two are combined into a Mantine theme plus a CSS
// variable resolver. Nothing here hand-writes component CSS.

import { createTheme, type CSSVariablesResolver, type MantineColorsTuple, type MantineThemeOverride } from '@mantine/core'
import { ensureContrast, oklchToRgb, parseHex, rgbToOklch, toHex } from './color'
import { resolvePalette, tokenToVariable, type Mode, type PaletteTokens } from './palettes'
import { resolveSkin } from './skins'

const sans = "'Geist Variable', system-ui, -apple-system, 'Segoe UI', sans-serif"
const mono = "'Geist Mono Variable', 'SFMono-Regular', Consolas, monospace"

/**
 * Mantine wants a ten-shade tuple. Ours is built so shade 6 is the light-mode
 * accent and shade 4 is the dark-mode accent — exactly the two shades
 * `primaryShade` selects — with the rest interpolated in OKLCH so the ramp stays
 * perceptually even.
 */
function accentRamp(paletteId: string): MantineColorsTuple {
  const light = parseHex(resolvePalette(paletteId, 'light').accent)
  const dark = parseHex(resolvePalette(paletteId, 'dark').accent)
  const lightOklch = rgbToOklch(light)
  const darkOklch = rgbToOklch(dark)
  const anchor = (lightness: number, chroma: number, hue: number) => toHex(oklchToRgb({ l: lightness, c: chroma, h: hue }))
  const hue = lightOklch.h
  const chroma = Math.max(lightOklch.c, darkOklch.c)
  const middle = (darkOklch.l + lightOklch.l) / 2
  const lower = (index: number) => Math.max(0.16, lightOklch.l - index * 0.07)
  return [
    anchor(0.97, Math.min(0.02, chroma * 0.2), hue),
    anchor(0.93, Math.min(0.04, chroma * 0.3), hue),
    anchor(0.88, Math.min(0.06, chroma * 0.45), hue),
    anchor(0.8, Math.min(0.09, chroma * 0.7), hue),
    anchor(darkOklch.l, Math.min(0.16, darkOklch.c), hue),
    anchor(middle, Math.min(0.16, chroma), hue),
    anchor(lightOklch.l, Math.min(0.16, lightOklch.c), hue),
    anchor(lower(1), Math.min(0.16, chroma), hue),
    anchor(lower(2), Math.min(0.15, chroma), hue),
    anchor(lower(3), Math.min(0.14, chroma), hue),
  ] as unknown as MantineColorsTuple
}

/**
 * Mantine's neutral ramps, re-anchored on the palette so greys never clash.
 * The anchors matter: Mantine reads `dark[6]` for dark-mode surfaces (a mid-grey
 * there washes out every card) and the `gray` shades for light-mode chrome, so
 * these follow Mantine's own lightness steps — with `dark[6]`/`dark[7]` pinned to
 * the palette's own surface and canvas, which is exactly what Mantine intends
 * its dark-6 and dark-7 to be.
 */
function neutralRamp(paletteId: string, scheme: Mode): MantineColorsTuple {
  const tokens = resolvePalette(paletteId, scheme)
  const hue = rgbToOklch(parseHex(tokens.canvas)).h
  const anchor = (lightness: number, chroma = 0.006) => toHex(oklchToRgb({ l: lightness, c: chroma, h: hue }))
  if (scheme === 'dark') {
    const canvasLightness = rgbToOklch(parseHex(tokens.canvas)).l
    return [
      anchor(0.82),
      anchor(0.76),
      anchor(0.58),
      anchor(0.48),
      anchor(0.38),
      anchor(0.31),
      tokens.surface,
      tokens.canvas,
      anchor(Math.max(0.06, canvasLightness - 0.03)),
      anchor(Math.max(0.03, canvasLightness - 0.06)),
    ] as unknown as MantineColorsTuple
  }
  return [anchor(0.98), anchor(0.96), anchor(0.94), anchor(0.89), anchor(0.85), anchor(0.74), anchor(0.61), anchor(0.38), anchor(0.3), anchor(0.22)] as unknown as MantineColorsTuple
}

/**
 * The Accessible & Ethical skin's identity is contrast, not decoration: text is
 * pushed to 7:1, control boundaries to 4.5:1 and hairlines to a visible edge.
 * Colours that already pass are left untouched.
 */
function applyHighContrast(tokens: PaletteTokens, high: boolean): PaletteTokens {
  if (!high) return tokens
  return {
    ...tokens,
    ink: ensureContrast(tokens.ink, tokens.canvas, 7),
    mutedInk: ensureContrast(tokens.mutedInk, tokens.canvas, 7),
    faintInk: ensureContrast(tokens.faintInk, tokens.canvas, 4.5),
    controlBorder: ensureContrast(tokens.controlBorder, tokens.canvas, 4.5),
    border: ensureContrast(tokens.border, tokens.surface, 1.5),
  }
}

export function buildTheme(skinId: string, paletteId: string, mode: Mode): MantineThemeOverride {
  const skin = resolveSkin(skinId)
  const tokens = resolvePalette(paletteId, mode)
  const headingFamily = skin.monoHeadings ? mono : skin.headings?.fontFamily ?? skin.fontFamily ?? sans
  const compact = skin.density === 'compact'
  // Materials a component library cannot express are attached as our own class
  // names through the theme, so `materials.css` never has to reach into
  // Mantine's hashed internal classes.
  const material = skin.material === 'flat' ? null : `pm-${skin.material}`
  const controlClass = material ? `${material}-control` : undefined
  const surfaceClass = material ? `${material}-surface` : undefined
  const chromeClass = material ? `${material}-chrome` : undefined
  // A skin's elevation is part of its identity (soft UI, clay, chrome, glass), but
  // Mantine only paints a shadow when a component asks for one — defining the
  // shadow scale alone left every surface flat.
  const surfaceShadow = skin.shadows.sm === 'none' ? undefined : 'sm'
  const elevatedShadow = skin.shadows.md === 'none' ? surfaceShadow : 'md'
  return createTheme({
    primaryColor: 'pangolin',
    primaryShade: { light: 6, dark: 4 },
    colors: {
      pangolin: accentRamp(paletteId),
      gray: neutralRamp(paletteId, "light"),
      dark: neutralRamp(paletteId, 'dark'),
    },
    fontFamily: skin.fontFamily ?? sans,
    fontFamilyMonospace: mono,
    headings: {
      fontFamily: headingFamily,
      fontWeight: skin.headings?.fontWeight ?? '640',
      sizes: { h1: { fontSize: '1.75rem', lineHeight: '1.2' }, h2: { fontSize: '1rem', lineHeight: '1.35' }, h3: { fontSize: '0.9375rem', lineHeight: '1.4' } },
    },
    fontSizes: { xs: '11px', sm: '12.5px', md: '14px', lg: '15.5px', xl: '18px' },
    lineHeights: { xs: '1.4', sm: '1.45', md: '1.5', lg: '1.55', xl: '1.5' },
    radius: skin.radius,
    defaultRadius: 'md',
    shadows: skin.shadows,
    spacing: compact
      ? { xs: '4px', sm: '6px', md: '10px', lg: '14px', xl: '20px' }
      : { xs: '6px', sm: '9px', md: '14px', lg: '20px', xl: '28px' },
    focusRing: skin.id === 'accessible' ? 'always' : 'auto',
    cursorType: 'pointer',
    components: {
      Button: {
        defaultProps: {
          radius: 'md',
          className: ['pm-button', controlClass].filter(Boolean).join(' '),
        },
        // Mantine resolves the filled label from `theme.white` and writes it as an
        // inline custom property, so no stylesheet rule can reach it. Theme vars
        // are merged after the component's own resolver, which makes this the one
        // channel that wins. Measured before it: #fff on the bronze accent was
        // 2.61:1, below WCAG AA.
        //
        // Only the filled variant may be touched: every other variant draws its
        // text from `--button-color` too, and forcing the accent contrast on them
        // put dark ink on a dark surface (measured 1.05:1). Mantine calls this
        // with the resolved props, so the variant is known here.
        vars: (_theme: unknown, props: { variant?: string }) =>
          (props.variant ?? 'filled') === 'filled'
            ? { root: { '--button-color': 'var(--accent-contrast)' } }
            : {},
      },
      ActionIcon: { defaultProps: { radius: 'md', className: controlClass } },
      TextInput: { defaultProps: { radius: 'md', className: controlClass } },
      NumberInput: { defaultProps: { radius: 'md', className: controlClass } },
      PasswordInput: { defaultProps: { radius: 'md', className: controlClass } },
      Textarea: { defaultProps: { radius: 'md', className: controlClass } },
      Select: { defaultProps: { radius: 'md', checkIconPosition: 'right', className: controlClass, comboboxProps: { shadow: 'md', radius: 'md' } } },
      MultiSelect: { defaultProps: { radius: 'md', checkIconPosition: 'right', className: controlClass, comboboxProps: { shadow: 'md', radius: 'md' } } },
      Modal: { defaultProps: { radius: 'lg', centered: true, shadow: elevatedShadow, classNames: { content: surfaceClass, body: surfaceClass }, overlayProps: { backgroundOpacity: 0.5, blur: skin.material === 'glass' ? 6 : 0 } } },
      Drawer: { defaultProps: { radius: 'lg', shadow: elevatedShadow, classNames: { content: surfaceClass } } },
      Tooltip: { defaultProps: { radius: 'sm', withArrow: true } },
      Badge: { defaultProps: { radius: 'sm' } },
      Card: { defaultProps: { radius: 'lg', withBorder: true, shadow: surfaceShadow, className: surfaceClass } },
      Paper: { defaultProps: { radius: 'lg', shadow: surfaceShadow, className: surfaceClass } },
      Menu: { defaultProps: { shadow: elevatedShadow, classNames: { dropdown: surfaceClass } } },
      Popover: { defaultProps: { shadow: elevatedShadow, classNames: { dropdown: surfaceClass } } },
      Table: { defaultProps: { highlightOnHover: true, verticalSpacing: compact ? 'xs' : 'sm', horizontalSpacing: 'md', className: surfaceClass } },
      Pagination: { defaultProps: { radius: 'md', size: 'sm' } },
      Checkbox: { defaultProps: { radius: 'sm' } },
      Switch: { defaultProps: { radius: 'xl' } },
      Alert: { defaultProps: { radius: 'lg', className: surfaceClass } },
      Tabs: { defaultProps: { radius: 'md' } },
      SegmentedControl: { defaultProps: { radius: 'md' } },
      AppShell: { defaultProps: { classNames: { navbar: chromeClass, header: chromeClass, main: 'pm-main' } } },
    },
    other: {
      // Consumed by the small material layer and by tests.
      pangolinMaterial: skin.material,
      pangolinMotion: skin.motion,
      pangolinDensity: skin.density,
      pangolinRowHeight: compact ? 44 : 54,
      pangolinTableFontSize: '14px',
      pangolinSkin: skin.id,
      pangolinPalette: paletteId,
    },
  })
}

/**
 * Maps Mantine's own variables onto the palette. Placement matters: Mantine
 * defines the text/body/default variables inside its light and dark blocks
 * (emitted under `[data-mantine-color-scheme]`, which outranks `:root`), so they
 * are repeated for both schemes here.
 */
export function buildResolver(paletteId: string, highContrast = false): CSSVariablesResolver {
  const light = applyHighContrast(resolvePalette(paletteId, 'light'), highContrast)
  const dark = applyHighContrast(resolvePalette(paletteId, 'dark'), highContrast)
  const scheme = (tokens: PaletteTokens) => ({
    '--mantine-color-bright': tokens.ink,
    '--mantine-color-text': tokens.ink,
    '--mantine-color-body': tokens.canvas,
    '--mantine-color-error': tokens.danger,
    '--mantine-color-success': tokens.success,
    '--mantine-color-placeholder': tokens.faintInk,
    '--mantine-color-anchor': tokens.accentStrong,
    '--mantine-color-default': tokens.surface,
    '--mantine-color-default-hover': tokens.surfaceMuted,
    '--mantine-color-default-color': tokens.ink,
    '--mantine-color-default-border': tokens.controlBorder,
    '--mantine-color-dimmed': tokens.mutedInk,
    '--mantine-color-disabled': tokens.surfaceMuted,
    '--mantine-color-disabled-color': tokens.faintInk,
    '--mantine-color-disabled-border': tokens.border,
    // These are scheme-dependent too: Mantine paints active nav items and light
    // buttons with `-light`/`-light-color`, so taking them from the light palette
    // in dark mode gave a pale fill under pale text — invisible labels.
    '--mantine-primary-color-filled': tokens.accent,
    '--mantine-primary-color-filled-hover': tokens.accentStrong,
    '--mantine-primary-color-contrast': tokens.accentContrast,
    '--mantine-primary-color-light': tokens.accentSoft,
    '--mantine-primary-color-light-hover': tokens.accentSoft,
    '--mantine-primary-color-light-color': tokens.accentStrong,
  })
  return () => ({
    variables: {
      '--mantine-radius-default': 'var(--radius-control)',
    },
    light: scheme(light),
    dark: scheme(dark),
  })
}

/** Our own token names, so CSS not yet migrated keeps agreeing with the palette. */
export function paletteVariables(paletteId: string, mode: Mode, highContrast = false): Record<string, string> {
  const tokens = applyHighContrast(resolvePalette(paletteId, mode), highContrast)
  const variables: Record<string, string> = {}
  for (const [token, value] of Object.entries(tokens)) {
    variables[tokenToVariable[token as keyof PaletteTokens]] = value
  }
  return variables
}

export function skinVariables(skinId: string, paletteId: string, mode: Mode): Record<string, string> {
  const skin = resolveSkin(skinId)
  const tokens = applyHighContrast(resolvePalette(paletteId, mode), Boolean(skin.highContrast))
  return {
    ...paletteVariables(paletteId, mode, Boolean(skin.highContrast)),
    '--radius-control': skin.radius.md,
    '--radius-panel': skin.radius.lg,
    '--radius-dialog': skin.radius.lg,
    '--radius-shell': skin.radius.xl,
    '--shadow-chrome': skin.shadows.md,
    '--shadow-raised': skin.shadows.sm,
    '--shadow-overlay': skin.shadows.lg,
    '--accent-contrast': tokens.accentContrast,
    '--focus-ring-width': skin.id === 'accessible' ? '3px' : '2px',
  }
}

export { sans, mono }
