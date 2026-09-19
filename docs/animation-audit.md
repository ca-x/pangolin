# Animation opportunity audit

This is the restrained motion pass requested for Pangolin / 鲮鲤. Every accepted item passed the frequency, purpose, speed and function gates before implementation.

## Part 1 — Opportunities

| # | Location | Today | Purpose | Frequency | Suggested motion |
| --- | --- | --- | --- | --- | --- |
| 1 | `web/src/styles.css:100` | Pressable controls need immediate acknowledgement | Feedback | Tens/day | Implemented `transform: scale(.97)` with an exact `transform 160ms cubic-bezier(.23,1,.32,1)` transition; hover motion is pointer-gated and reduced motion removes the transform. |
| 2 | `web/src/styles.css:179` | Select content otherwise appears without a relationship to its trigger | Spatial consistency | Occasional | Implemented trigger-origin `opacity` + `scale(.97→1)` over 180ms using `--ease-out`; exit reverses over 140ms. |
| 3 | `web/src/styles.css:152` | Create/reveal dialogs otherwise teleport above dense content | Preventing a jarring change | Occasional | Implemented centered `opacity` + `scale(.97→1)` over 220ms using `--ease-out`; exit reverses over 160ms. Modals deliberately retain a centered transform origin. |
| 4 | `web/src/styles.css:160` | Request detail needs a clear relationship to the right edge | Spatial consistency | Occasional | Implemented `translateX(100%→0)` + opacity over 260ms with `--ease-out`; exit returns through the same edge over 180ms. |
| 5 | `web/src/styles.css:242` | Mobile navigation needs to explain where the temporary navigation surface lives | Spatial consistency | Occasional on mobile | Implemented edge-aligned drawer movement over 260ms using `cubic-bezier(.32,.72,0,1)` with a matched scrim, focus containment and a reduced-motion static alternative. |

All accepted motion is capped below 300ms and uses transform/opacity only. The global reduced-motion rule is at `web/src/styles.css:256`.

## Part 2 — Rejected candidates

- `web/src/Pages.tsx:45` — animated request chart drawing. **Rejected: functional data the operator is reading; movement hinders comparison.** The SVG renders immediately and has a table fallback.
- `web/src/Shell.tsx` — route/page transitions. **Rejected: core navigation is used tens to hundreds of times per day.** Content updates immediately.
- Status indicators in request tables — continuous pulse. **Rejected: no state-information gain and persistent motion distracts in a monitoring surface.** Status uses icon/text/color together.
- Keyboard focus and shortcut navigation — animated focus travel. **Rejected: keyboard-initiated and high frequency. Never animate.** Focus changes immediately with a visible ring.
- Dashboard cards — staggered entrance on every overview visit. **Rejected: daily-use information should be ready to scan, not wait for decoration.**

## Part 3 — Verdict

Pangolin needs very little motion. The highest-leverage item is the request-detail drawer because it preserves spatial context while moving from summary to diagnosis. The implementation is intentionally below the available “delight budget”; first-run completion remains the only future location where a short one-time flourish would be justified.
