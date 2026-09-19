# Pangolin design system

> Product-wide source of truth. Page files under `pages/` may only override rules explicitly named there.

## Direction

Pangolin（中文正式名称“鲮鲤”）is a high-trust developer operations console: compact, calm, tactile and exact. The supplied logo contributes graphite scales, warm copper edges and an amethyst detail. The UI echoes those materials through restrained color and fine borders, not skeuomorphic cards or ornamental gradients.

The initial UI/UX Pro Max result was narrowed after a dashboard-specific retry. We keep its dense operations layout, real-time status labeling, keyboard requirements and technical typography guidance; we reject its glassmorphism/scroll-storytelling recommendation because persistent blur and showy motion reduce clarity in a daily-use data product.

## Typography

- UI: `Geist Sans`, with `Inter`, `Segoe UI` and system sans fallbacks.
- Code, ids and numeric telemetry: `Geist Mono`, `SFMono-Regular`, `Cascadia Code` and monospace fallbacks.
- Headings use the same family as body copy with tighter tracking and stronger weight. No fashion serif.
- Body text is at least 14px in dense tables and 16px in forms/long copy; line height is 1.5.

## Core tokens

Light mode:

| Role | Value |
| --- | --- |
| canvas | `#f4f2ee` |
| surface | `#fbfaf8` |
| elevated | `#ffffff` |
| ink | `#1d1a19` |
| muted ink | `#696360` |
| border | `#dcd6d0` |
| accent | `#985c3f` |
| accent strong | `#74412e` |
| amethyst | `#76566f` |
| success | `#2f725b` |
| warning | `#9a681e` |
| danger | `#b43c36` |

Dark mode:

| Role | Value |
| --- | --- |
| canvas | `#151312` |
| surface | `#1d1a19` |
| elevated | `#252120` |
| ink | `#f1ece7` |
| muted ink | `#aaa19b` |
| border | `#3a3431` |
| accent | `#d08a66` |
| accent strong | `#e5a27d` |
| amethyst | `#b994b0` |
| success | `#6eb092` |
| warning | `#d2a154` |
| danger | `#e27972` |

Accent themes swap only accent/amethyst/status-adjacent tokens: `bronze` (default), `slate`, `jade`. Semantic error/success meaning never changes with theme.

## Geometry and density

- Spacing scale: 4, 8, 12, 16, 24, 32px.
- Control height: 40px desktop, minimum interactive hit area 44×44px.
- Radius: 8px controls, 12px panels, 16px dialogs. Avoid pill shapes except compact statuses.
- Shadows are reserved for overlays: `0 18px 50px rgb(20 14 12 / 0.18)`. Panels use borders, not floating-card shadows.
- Desktop shell: 248px navigation rail, fluid content max 1600px. Mobile shell uses a top bar and modal navigation.

## Components

- Buttons provide a subtle `scale(0.97)` active state over 160ms and never move vertically on hover.
- Inputs always keep visible labels, helper/error text space and a 2px focus ring with 2px offset.
- Tables use sticky headings only when they do not obscure focused content; every row action has a text label or accessible name.
- Status combines text, icon/shape and color. No unlabeled glowing dot.
- Charts use SVG for bounded data, have a visible current/trend summary and a table/list fallback. Data does not animate.
- Dialogs, menus, selects and tooltips use Radix primitives for focus management and keyboard behavior.

## Motion vocabulary

```css
--ease-out: cubic-bezier(0.23, 1, 0.32, 1);
--ease-in-out: cubic-bezier(0.77, 0, 0.175, 1);
--ease-drawer: cubic-bezier(0.32, 0.72, 0, 1);
```

- Press feedback: transform only, 160ms.
- Popover/tooltip: opacity + `scale(0.97)`, trigger-aware origin, 180ms ease-out.
- Dialog: opacity + `scale(0.97)`, centered origin, 220ms ease-out; exit 160ms.
- Toast: opacity + vertical percentage transform, 220ms ease-out; exits through the same edge.
- Setup completion: one-time scale `0.95` + opacity with a 50ms short stagger; no bounce.
- No page transitions, animated charts, command-menu animation, continuous pulse, or hover motion on coarse pointers.
- Under `prefers-reduced-motion: reduce`, remove transform movement while retaining short opacity/color feedback.

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
