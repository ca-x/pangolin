# Handover: remaining AxonHub parity work

Self-contained execution brief for whoever leads the rest of this work. Read
`AGENTS.md` first — it is binding — then `docs/reviews/parity-ledger.md`, which is
the durable record and carries the evidence for everything marked closed, and
`tasks/parity-completion-plan.md`, which is the ordered batch plan this document
hands over to.

## Objective

Close the AxonHub experience gap in the Pangolin console. Done means four things,
quoted from the goal:

1. the console completes its own workflows end to end — backup/restore, jobs,
   catalog sync, access grants, playground runs;
2. it never states something untrue — no invented zeros, no lying status, no silent
   payload deletion;
3. destructive and destructive-adjacent flows are guarded by component dialogs;
4. parity claims are verifiable against the reports rather than asserted.

**Scope was expanded by the user after this document was first written.** It is no
longer limited to the wiring defects below: it covers **all AxonHub-aligned
functionality and UI interactions**, including everything the six area reports
called `feature-build` and everything this document previously deferred. The
authoritative list of what remains is the reconciled ledger (54 rows need work:
12 `partial`, 31 `open`, 11 `feature-build`) plus the 113-item capability-matrix
disposition in the same file.

## Reference policy (non-negotiable)

`/home/czyt/code/others/axonhub` (Apache-2.0/LGPL) is a **behaviour and test
reference**: port observable contracts, never copy Go implementation or assets.
`QuantumNous/new-api` is AGPL-3.0: study product behaviour only, never copy code or
image assets. Where the reference conflicts with this repository's architecture,
follow `AGENTS.md` and record the divergence in the ledger's
[Intentional divergences](../docs/reviews/parity-ledger.md#intentional-divergences)
table.

## Verified state at handover (2026-09-22)

| Gate | Result |
| --- | --- |
| `cargo test --locked` | 242 passed, 0 failed |
| `pnpm --dir web test` | 58 files / 1333 passed (in isolation) |
| `cargo fmt --all -- --check`, clippy `-D warnings` | clean (last recorded run, Tasks D2b) |
| `pnpm --dir web lint`, `pnpm --dir web build` | clean (last recorded run, Tasks C2/D2b) |
| Branch / HEAD | `main` at `e666bef`, three commits ahead of `origin/main` (`ae052b4`), unpushed |
| Working tree | 52 modified tracked paths + 59 untracked files, nothing staged |

**The suite is load-flaky and the binary embeds `web/dist` at compile time.**
A web run overlapping `cargo test` reported 8 failures that did not reproduce in
isolation on the same tree. And three separate QA runs were wasted on a stale
binary: build web assets *before* the release binary, every time.

## What is already closed

Every defect this document originally listed as "remaining" is now fixed, and the
evidence lives in the ledger under
[Closed since the area reports](../docs/reviews/parity-ledger.md#closed-since-the-area-reports)
and in the per-task reports in `.superpowers/sdd/parity-remaining-handover/`.
Specifically:

| Original item | State | Where |
| --- | --- | --- |
| 1. Stream usage on the `finish_reason` chunk reported as failed | **closed** | Task B report; `lifecycle.rs` OpenAI arm; mock-upstream test asserts 200, `11/7` tokens and the exact charge |
| 2. Playground shows no usage or cost | **closed** | Task C2 report; the record is read after a terminal outcome and rendered with `—` when unmeasured |
| 3. Catalog source row stale after a failed refresh | **closed** | Task C1 report; both `refresh` and `refreshDue` invalidate on HTTP failure |
| 4. Two badge contrast failures | **open — reproduce first** | measured on a stale bundle; see C7 below |
| 5. English data-table clipping at 375px | **open — reproduce first** | never reproduced locally; see C8 below |
| 6. Routing field implemented but unguarded | **open** | the field is real and submits; only its guard is missing; see C6 below |

Also closed, because they were found while executing the above: the
request-detail identity bug (A1), the generic `bulk-toggle` API-key bypass (A2),
foreign-resource delete enumeration (A3), the authoritative rebuild's invented
`200`/`502` and provider-id-as-name (D2b), the strict-write/tolerant-read logging
policy split (D1), stable project-scoped observability identity (D2a), and the
playground's terminal classification (C2).

## What still needs work

The reconciled ledger is the list. Read it per area:

- `partial` (12) — a capability exists but a named part of the workflow is missing.
- `open` (31) — real and untouched.
- `feature-build` (11) — nothing exists on the Pangolin side.
- `divergence` (2) — deliberate; do not "fix" them.
- `decisions required` (6) — a product decision is the deliverable, not code.

The expanded scope adds three whole families that this document used to defer and
that are now in scope:

1. **Model catalog, cards and discovery** — applying catalog cards to
   console-created models, a global model record with card fields and an archive
   lifecycle, `/v1/models` metadata, global model settings, provider model
   discovery/sync.
2. **Pricing** — channel-scoped prices, the components read projection, a volume
   tier mode, the service-group editor, and the full price editor (tiers,
   schedules, timezones).
3. **Instance and platform settings** — request-logging `version`, schedule
   cron/time-of-day/timezone, instance general/retry/quota settings, typed storage
   backends with a connection test, diagnostics, outbound proxy presets and
   per-webhook timeout/proxy, plus the provider OAuth helper flows
   (Codex/xAI/Claude Code/Antigravity/Copilot).

## Deferred corrections that are still open

These are not new findings; each is named in the ledger's
[Deferred corrections](../docs/reviews/parity-ledger.md#deferred-corrections-carried-forward)
table with its required guard.

| # | Correction | Why it is still open |
| --- | --- | --- |
| C1 | Access project-switch loading flash | `ProjectProvider` hands out an empty permission set while the next project's permissions load, so the page renders "no access" and unmounts panel state |
| C2 | "Saved but the list could not be re-read" emits two toasts | the success path reports success and then reports the refetch failure separately |
| C3 | `TraceTimeline` links by the external request id | the bundle also carries the internal `request.id`, which is the identity that cannot collide |
| C4 | Playground links by the external request id | the gateway's `x-request-id` header is not `requests.id`; the record read returns the internal id |
| C5 | The D1 test comment is inaccurate | it says an unknown enum "fell back to `metadata`"; D1's own RED evidence shows that shape was answered 422 by the extractor, and only an unknown *field* silently defaulted |
| C6 | The routing form has no guard | the panel lives in a `keepMounted={false}` tab whose default is `appearance`; a test that does not activate the tab mounts nothing, which is also why the sibling field never rendered |
| C7 | Badge contrast unmeasured | the 4.32:1 / ~2.68:1 numbers came from a stale bundle; reproduce before changing any colour |
| C8 | 375px English table clipping | reported, never reproduced; reproduce before fixing |

C5–C8 are the four items the handover flagged as "do not start without evidence"
and they keep that rule: **C7 and C8 are reproduce-before-fix**, and a stale
measurement is not a licence to change colours or widths.

## Method, and why it is not optional here

- **Red/green for every behaviour change.** A test written after the fix is not
  evidence. If you cannot make it fail first, say so explicitly. This work has
  produced **four** cases where a test passed because it asserted against a mock
  that did not match the API — check the shape the API actually sends: the code
  lives in `error.type` on the OpenAI envelope, not `error.code` (that is Gemini's
  numeric status); the request list exposes the internal id as `internal_id`; the
  playground reads the record by the id the gateway returned, not by a token.
- **Gates before any claim:** the three Rust commands and the three web commands in
  `AGENTS.md` → Build and verification, plus focused tests for each change.
- **Browser verification is required** for console-facing work: 375/768/1440, light
  and dark, `zh-CN` and `en`, against the release binary. Working recipe: provider
  `openai_compatible` against a mock upstream; models; **an association**; and
  crucially **a credential** — without one every gateway request returns
  `400 no eligible route`. Create a key with `POST /api/admin/v1/api-keys` →
  `{"key":…,"mode":"generated","token":…}`; log in with
  `POST /api/v1/auth/login` (cookie `pangolin_session`); mutations need
  `x-pangolin-csrf: 1`. **Ports 18086-18089 and 18094-18099 are taken by earlier
  runs, as are `/tmp/qa-*`** — pick a free port and a fresh data dir.
- **Rebuild web assets, then the release binary, as one step.** The binary embeds
  `web/dist` at compile time.
- If a full test run looks flaky, re-run the specific files and check file mtimes
  before attributing a failure. This suite has shown load flakiness under parallel
  work, and one earlier attribution to "another agent" was refuted by mtimes.
- Do not commit unless asked. Do not use destructive git commands.

## Ordered steps

1. Re-run all six gates and confirm the handover state above still holds.
2. Read `tasks/parity-completion-plan.md` and execute its batches in order. The
   first batch is the reproduce-first measurement pass (C7, C8), because it decides
   whether those two corrections need code at all.
3. Land the small identity and state-feedback corrections (C1–C6) before the larger
   capability batches, so the console's own truthfulness is not still moving while
   new surfaces are added.
4. Work the ledger's `partial` and `open` rows per area, then the `feature-build`
   rows, then the matrix-level rows that no area report covered (provider OAuth
   helpers, GraphQL, CORS/timeouts, live preview).
5. Take the six `decisions required` rows to the user as decisions, not as code.
6. Rebuild web assets and the release binary, run the full browser pass over the
   workflows the goal names, and record every closure in
   `docs/reviews/parity-ledger.md` in its existing style: what was wrong, the
   `file:line`, the red output verbatim, the green output, and the gates.
7. Keep the ledger's counts honest: when a row closes, move it and re-derive the
   counts table rather than editing the total.

## Known risks

- **A stale binary will make you report defects that are already fixed, or miss
  ones that are new.** Rebuild web assets and the binary as one step.
- **Stale reports mislead.** `docs/reviews/parity-status.md` contradicts the code
  on 17 rows and is superseded by the ledger. The six area reports are
  point-in-time evidence; verify against the code, not the prose.
- **A test can prove the mock rather than the product.** Four instances are
  recorded in the ledger; every new test should assert the shape the API actually
  sends.
- **The fix that looks obvious may be the wrong side.** Twice in this work the
  first diagnosis was wrong: an upstream-status mapping that was already correct,
  and a request-detail 404 that came from the row fetch rather than the pre-check
  it looked like. Follow the value to where it is actually consumed.
