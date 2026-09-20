# Pangolin design system

> Product-wide source of truth. Page files under `pages/` may only override rules explicitly named there.

## Direction

Pangolin（中文正式名称“鲮鲤”）is a high-trust developer operations console: layered, calm, tactile and exact. The supplied logo contributes graphite scales, warm copper edges and an amethyst detail. The UI echoes those materials through layered warm surfaces, hairline borders and a single accent — never skeuomorphic cards, ornamental gradients or glassmorphism.

Two references set the current bar. **AxonHub** (Apache-2.0, behaviour reference) established the refinement targets: floating navigation chrome, layered warm surfaces, one accent used sparingly, generous rhythm, monospace for machine data, and minimal single-series charts. **Apple's fluid-interface principles** contributed response-on-press, symmetric enter/exit paths, size-specific type tracking, translucent chrome with a reduced-transparency fallback, and scroll-edge fades instead of hard dividers. Pangolin keeps its own identity — compact operations density, bronze/slate/jade accents, dark-mode parity, bilingual copy — and exceeds the reference on accessibility rigour: every animation respects `prefers-reduced-motion`, every status carries text plus shape plus color, and no data surface depends on hover.

Rejected: glassmorphism card stacks, atmospheric gradients, decorative 01/02/03 labels, animated charts, page transitions, and command-menu animation. Daily-use data products lose clarity to showy motion.

## Typography

- UI: `Geist Sans`, with `Inter`, `Segoe UI` and system sans fallbacks.
- Code, ids, numeric telemetry: `Geist Mono`, `SFMono-Regular`, `Cascadia Code` and monospace fallbacks, `font-feature-settings: 'zero' 1`.
- Metric values (stat cards, trace outcomes, cost) use the mono family with `font-variant-numeric: tabular-nums` so digits align column to column.
- Headings use the same family as body copy with tighter tracking and stronger weight: `h1` is `clamp(1.55rem, 2.2vw, 1.95rem)` at weight 640 with `-0.028em`; `h2` is 1rem at weight 620 with `-0.014em`. Tracking tightens as size grows; body copy stays near zero.
- Body text is at least 14px in dense tables and 16px in forms/long copy; line height is 1.5 (1.55 in code blocks).
- Micro labels (nav groups, table headers, stat captions, definition terms) are 10.5–11.5px, weight 560–660, uppercase, `0.07–0.085em` tracking, in `--faint`.

## Core tokens

Light mode:

| Role | Value |
| --- | --- |
| canvas | `#f2efe9` |
| surface (cards) | `#ffffff` |
| surface muted (insets, table headers) | `#f7f4ef` |
| elevated (overlays) | `#ffffff` |
| ink | `#1c1917` |
| muted ink | `#6b635c` |
| faint ink | `#8e857d` |
| border | `#e6e1d8` |
| border subtle | `#efeae1` |
| control border | `#c9c0b6` |
| accent | `#9a5a38` |
| accent strong | `#7b4227` |
| accent soft | `#f5eae1` |
| accent contrast | `#ffffff` |
| amethyst | `#7a5872` |
| success / soft | `#2c6f57` / `#e2efe8` |
| warning / soft | `#96631a` / `#f7ecd7` |
| danger / soft | `#b23a34` / `#f9e6e3` |

Dark mode:

| Role | Value |
| --- | --- |
| canvas | `#131110` |
| surface | `#1c1917` |
| surface muted | `#221e1c` |
| elevated | `#272220` |
| ink | `#f3eeea` |
| muted ink | `#a79e98` |
| faint ink | `#857b76` |
| border | `#332e2b` |
| border subtle | `#292422` |
| control border | `#574e4a` |
| accent | `#d78f6b` |
| accent strong | `#eaa87f` |
| accent soft | `#3a2620` |
| accent contrast | `#1b1210` |
| success | `#6cb091` |
| warning | `#d2a154` |
| danger | `#e57f78` |

Accent themes swap only accent/amethyst/status-adjacent tokens and `--accent-contrast`: `bronze` (default), `slate`, `jade`. Semantic error/success meaning never changes with theme.

Surfaces are strictly layered — `canvas < surface-muted < surface < elevated`. Content sits on `surface` (white in light mode); insets (code blocks, table headers, JSON bodies) sit on `surface-muted`; overlays sit on `elevated`. Never place a translucent surface on another translucent surface.

## Geometry and density

- Spacing scale: 4, 8, 12, 16, 24, 32px.
- Control height: 40px desktop (`--control-height`), expanded to 44px on coarse pointers so touch keeps the minimum hit area. Icon buttons are 40px square (44px on touch); in-table row actions are 36px.
- Radius ladder: 9px controls, 10px nav items and icon chips, 12–14px panels and tables, 16px dialogs, 18px auth cards and the sidebar shell. Pills (status, switches) are fully rounded. Avoid mixing radii inside one surface.
- Elevation is reserved for things that float: `--shadow-chrome` (sidebar, sticky chrome), `--shadow-overlay` (dialogs, drawers, popovers, toasts). Cards and tables use hairline borders, not shadows.
- Desktop shell: 244px floating navigation panel inset 12px from the viewport edge, fluid content capped at 1560px. Mobile shell uses a translucent top bar and a modal drawer.

## Components

- Buttons provide a `scale(0.97)` active state over 160ms on every pointer type, never move vertically on hover, and carry a 1px contact shadow. Primary buttons fill with `--accent` and use `--accent-contrast` text so dark-mode contrast holds.
- Inputs always keep visible labels, helper/error text space and a 3px soft focus ring at 18% accent-mix. Radix selects and native selects share that ring. Booleans render as iOS-style switches — the native checkbox keeps its semantics and FormData name while CSS paints a 34×20px track with a 180ms thumb slide.
- Navigation items are 40px tall with a 10px radius; the active item is a filled accent pill with `--accent-contrast` text and an inset highlight. Group labels are 10.5px uppercase.
- Tables use a sticky header inside the scroll region (`max-height: min(74vh, 900px)`), a `surface-muted` header band with blur, hairline row separators, 54px rows, and a `surface-muted` row hover. Machine fields use the mono family. Every row action has a text label or accessible name plus a tooltip.
- Status combines text, icon/shape and color in a pill (`--success-soft`/`--danger-soft` background, colored icon chip). No unlabeled glowing dot.
- Charts are hand-built SVG: dashed horizontal gridlines only, a 26%→0 accent area fill, a 2px accent line, axis labels in `--faint`, and a hover crosshair with an elevated tooltip. Data never animates; the data table fallback stays under every chart.
- Dialogs, menus, selects, tooltips and the toast host use Radix/sonner primitives for focus management, keyboard behaviour and portal layering. Tooltips render as inverted ink chips with a trigger-aware origin.

## Motion vocabulary

```css
--ease-out: cubic-bezier(.23, 1, .32, 1);
--ease-in-out: cubic-bezier(.77, 0, .175, 1);
--ease-drawer: cubic-bezier(.32, .72, 0, 1);
```

- Press feedback: transform only, 160ms, on fine and coarse pointers alike.
- Nav and button colour/background: 140–160ms ease.
- Popover/tooltip/select: opacity + `scale(0.97)`, trigger-aware origin, 180ms in / 140ms out.
- Dialog: opacity + `scale(0.97)`, centered origin, 220ms in / 160ms out.
- Drawer: opacity + horizontal percentage transform, 260ms `--ease-drawer` in / 200ms out — the same edge both ways.
- Toast: opacity + vertical percentage transform, 220ms ease-out; exits through the same edge.
- Skeletons sweep a muted gradient at 1.6s linear, never pulse the whole layout.
- Setup completion: one-time scale `0.95` + opacity with a 50ms short stagger; no bounce.
- No page transitions, animated charts, command-menu animation, continuous pulse, or hover motion on coarse pointers.
- Under `prefers-reduced-motion: reduce`, transform movement is removed while short opacity/colour feedback stays. Under `prefers-reduced-transparency: reduce`, blurred chrome becomes opaque. Under `prefers-contrast: more`, borders and faint ink strengthen.

## Accessibility and responsive rules

- WCAG 2.2 AA contrast, visible focus, skip link, semantic landmarks and logical DOM/tab order.
- Modals trap focus and restore it to the trigger. Sticky UI must not fully cover focused controls.
- All functionality works at 375, 768, 1024 and 1440px without horizontal page scrolling.
- Tables become horizontally contained regions with an accessible label; key facts remain visible as compact cards on narrow screens.
- Loading, empty, stale and error states are textual. Live telemetry exposes update time and a pause control if updates become frequent.

## Anti-patterns

- No glassmorphism, backdrop-blur card stacks, atmospheric gradients or oversized shadows.
- No gradient-clipped headlines, emoji icons, badge spam or decorative 01/02/03 labels.
- No `transition: all`, `scale(0)`, built-in `ease-in`, or UI animation over 300ms.
- No placeholder-only labels, gray-on-gray low contrast, or color-only status.
- No more than one accent hue per screen; status colours appear as small fills, never as large blocks.
