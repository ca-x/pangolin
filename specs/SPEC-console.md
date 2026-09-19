# Spec: console

## Objective

Provide a precise, calm operations console that makes setup and daily diagnosis fast. It must remain legible in dense views and polished without decorative motion competing with data.

## Stack

- React 19, TypeScript and Vite
- React Router, TanStack Query/Table
- Radix primitives for accessible dialogs, menus, selects and tooltips
- Recharts for bounded dashboard trends; every chart has a textual/table fallback
- i18next for Simplified Chinese and English

## Information architecture

- Setup / sign in
- Overview
- Providers
- Models and routes
- API keys
- Requests and request detail
- Settings

## Visual and interaction requirements

- Warm graphite/bronze visual language derived from the supplied pangolin logo; no generic gradient hero, glass-card pileup or emoji icons.
- Theme mode: system, light and dark. Accent theme: bronze, slate and jade.
- Minimum 44px touch targets, persistent visible labels, visible focus, skip link and logical keyboard order.
- Responsive at 375, 768, 1024 and 1440px with no horizontal page scroll.
- Motion is restricted to feedback, state continuity and rare setup success. Reduced-motion removes transforms.

## Animation gate

- Include: button press feedback (160ms), origin-aware popovers (180ms), occasional dialog transition (220ms), setup completion delight (one-time only).
- Reject: animated charts, route/page transitions, keyboard navigation animation and continuously pulsing status dots.

## Success criteria

- Language, color mode and accent can be changed without reload and persist per user/browser.
- All core actions work by keyboard and meet WCAG 2.2 AA contrast/focus expectations.
- Empty, loading, error and success states are explicit and do not shift layout unexpectedly.
- The console builds to static assets embedded in the Rust release binary.
