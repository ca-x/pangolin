// Colour maths for the theme system. No dependencies: OKLCH gives us
// perceptually even lightness steps, which is what makes a derived dark mode
// look deliberate rather than mixed, and WCAG contrast lets the test suite
// prove every skin × palette combination instead of trusting an eye.

export type Rgb = { r: number; g: number; b: number }
export type Oklch = { l: number; c: number; h: number }

export function parseHex(hex: string): Rgb {
  const value = hex.trim().replace('#', '')
  const full = value.length === 3 ? value.split('').map((c) => c + c).join('') : value
  if (!/^[0-9a-f]{6}$/i.test(full)) throw new Error(`not a hex colour: ${hex}`)
  return {
    r: parseInt(full.slice(0, 2), 16) / 255,
    g: parseInt(full.slice(2, 4), 16) / 255,
    b: parseInt(full.slice(4, 6), 16) / 255,
  }
}

export function toHex({ r, g, b }: Rgb): string {
  const channel = (value: number) => Math.round(Math.min(1, Math.max(0, value)) * 255).toString(16).padStart(2, '0')
  return `#${channel(r)}${channel(g)}${channel(b)}`
}

const srgbToLinear = (value: number) => (value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4)
const linearToSrgb = (value: number) => (value <= 0.0031308 ? value * 12.92 : 1.055 * value ** (1 / 2.4) - 0.055)

export function rgbToOklch({ r, g, b }: Rgb): Oklch {
  const lr = srgbToLinear(r)
  const lg = srgbToLinear(g)
  const lb = srgbToLinear(b)
  const l = Math.cbrt(0.4122214708 * lr + 0.5363325363 * lg + 0.0514459929 * lb)
  const m = Math.cbrt(0.2119034982 * lr + 0.6806995451 * lg + 0.1073969566 * lb)
  const s = Math.cbrt(0.0883024619 * lr + 0.2817188376 * lg + 0.6299787005 * lb)
  const L = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s
  const A = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s
  const B = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s
  const chroma = Math.sqrt(A * A + B * B)
  const hue = chroma < 1e-6 ? 0 : (Math.atan2(B, A) * 180) / Math.PI
  return { l: L, c: chroma, h: (hue + 360) % 360 }
}

export function oklchToRgb({ l, c, h }: Oklch): Rgb {
  const radians = (h * Math.PI) / 180
  const A = c * Math.cos(radians)
  const B = c * Math.sin(radians)
  const l_ = (l + 0.3963377774 * A + 0.2158037573 * B) ** 3
  const m_ = (l - 0.1055613458 * A - 0.0638541728 * B) ** 3
  const s_ = (l - 0.0894841775 * A - 1.291485548 * B) ** 3
  return {
    r: linearToSrgb(4.0767416621 * l_ - 3.3077115913 * m_ + 0.2309699292 * s_),
    g: linearToSrgb(-1.2684380046 * l_ + 2.6097574011 * m_ - 0.3413193965 * s_),
    b: linearToSrgb(-0.0041960863 * l_ - 0.7034186147 * m_ + 1.707614701 * s_),
  }
}

/** Re-light a colour in OKLCH: keeps the hue, clamps chroma into sRGB. */
export function withLightness(hex: string, lightness: number, chromaCap?: number): string {
  const { c, h } = rgbToOklch(parseHex(hex))
  let chroma = Math.min(chromaCap ?? c, c)
  // Chroma that leaves the sRGB gamut gets clipped on the way back; stepping it
  // down keeps the hue honest instead of letting a channel clamp distort it.
  for (let attempt = 0; attempt < 12; attempt += 1) {
    const candidate = oklchToRgb({ l: lightness, c: chroma, h })
    const inside = [candidate.r, candidate.g, candidate.b].every((channel) => channel >= -0.001 && channel <= 1.001)
    if (inside) return toHex(candidate)
    chroma *= 0.88
  }
  return toHex(oklchToRgb({ l: lightness, c: 0, h }))
}

export function relativeLuminance(hex: string): number {
  const { r, g, b } = parseHex(hex)
  return 0.2126 * srgbToLinear(r) + 0.7152 * srgbToLinear(g) + 0.0722 * srgbToLinear(b)
}

/** WCAG 2.1 contrast ratio, 1–21. */
export function contrastRatio(a: string, b: string): number {
  const first = relativeLuminance(a)
  const second = relativeLuminance(b)
  const lighter = Math.max(first, second)
  const darker = Math.min(first, second)
  return (lighter + 0.05) / (darker + 0.05)
}

/** Black or white, whichever reads better on the given colour. */
export function readableInk(background: string): string {
  return contrastRatio('#ffffff', background) >= contrastRatio('#000000', background) ? '#ffffff' : '#000000'
}

/**
 * The nearest lightness that reaches a contrast target against `background`,
 * keeping the hue. Searches from the background outward, so a colour that only
 * just misses the bar moves the minimum distance instead of turning black.
 */
export function atContrast(hex: string, background: string, target: number): string {
  const { c, h } = rgbToOklch(parseHex(hex))
  const chroma = Math.min(c, 0.16)
  const towardDark = relativeLuminance(background) > 0.18
  let low = 0
  let high = 1
  let best = hex
  for (let attempt = 0; attempt < 26; attempt += 1) {
    const middle = (low + high) / 2
    const candidate = toHex(oklchToRgb({ l: middle, c: chroma, h }))
    const reaches = contrastRatio(candidate, background) >= target
    if (reaches) {
      best = candidate
      if (towardDark) low = middle
      else high = middle
    } else if (towardDark) {
      high = middle
    } else {
      low = middle
    }
  }
  return best
}

/** Keeps a colour as authored when it already passes, correcting only failures. */
export function ensureContrast(hex: string, background: string, target: number): string {
  return contrastRatio(hex, background) >= target ? hex.toLowerCase() : atContrast(hex, background, target)
}
