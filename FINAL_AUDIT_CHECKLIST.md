# Nightshift final prototype audit checklist

Run this after the roadmap is complete and before declaring the prototype phase finished.

The goal is not to add another feature wave. The goal is to make the repository boringly coherent: one source of truth per concern, bounded work, understood failure modes, clean ownership, and no clipboard goblins quietly running production from a broom closet.

## Audit rules

- [ ] Start from the current canonical `prototype/integrated-mvp` head and record its SHA.
- [ ] Freeze feature work while the audit is running; fixes should be small, reviewable cleanup slices.
- [ ] For every cleanup slice, inspect the complete diff before CI.
- [ ] Trace standalone integration-test module graphs whenever source-module dependencies change.
- [ ] Require completed green CI before promoting each audit fix.
- [ ] Fetch canonical immediately before every promotion and verify ancestry before moving refs.
- [ ] Do not preserve compatibility code merely because it exists; if a replacement is canonical, remove the old ledger/path.
- [ ] Do not turn audit findings into speculative frameworks. Fix the concrete failure class.

## 1. Repository and branch hygiene

- [ ] Inventory all remaining branches and classify each as canonical ancestor, active work, intentionally retained reference, or safe-to-delete.
- [ ] Compare every non-obvious old prototype/refactor branch against canonical before deciding it is disposable.
- [ ] Remove obsolete scratch files, temporary markers, abandoned experiment docs, and accidental generated artifacts.
- [ ] Verify no stale branch is still treated as the source of truth in docs, comments, scripts, or issue text.
- [ ] Check that canonical contains every intended promoted slice and no accidental/no-op commits that should be excluded from the production baseline.
- [ ] Review root-level files for obsolete prototype instructions or duplicated documentation.
- [ ] Make the final branch ownership story obvious to a new contributor.

## 2. Architecture and module boundaries

- [ ] Confirm each major concern has one clear owner: daemon state, protocol, VRChat API, VRChat worker, overlay/UI, OSC, HR, CLI/diagnostics.
- [ ] Hunt for duplicate state ledgers, mirrored booleans, shadow caches, or parallel geometry bookkeeping.
- [ ] Remove compatibility shims whose replacement is already canonical.
- [ ] Check for modules that became grab-bags during prototype work; split only where ownership is genuinely unclear.
- [ ] Verify feature-local state machines remain feature-local instead of leaking into generic manager abstractions.
- [ ] Check that UI remains mostly dumb and does not quietly own policy or automation decisions.
- [ ] Confirm renderer/layout, interaction, application state, OpenXR, COSMIC Text, effects, and Hydra boundaries are still intentional.
- [ ] Search for copy/paste helper variants that should now share one small typed boundary.

## 3. Async, concurrency, and generation safety

- [ ] Inventory every spawned task, `JoinSet`, channel, timer, and background refresh loop.
- [ ] Confirm every long-lived async result has a credible stale-result policy.
- [ ] Verify VRChat work cannot write across logout, re-authentication, or account/session replacement.
- [ ] Verify cache writes cannot cross session/account generations.
- [ ] Confirm pending status/invite/refresh work is cancelled or invalidated when its owning state changes.
- [ ] Check bounded queues for sensible capacity and overflow/latest-wins behavior.
- [ ] Identify any unbounded channels and justify them or replace them with a bounded/latest-wins mechanism where appropriate.
- [ ] Confirm timers use skipped/coalesced ticks where backlog would be harmful.
- [ ] Verify shutdown/drop paths abort owned tasks and do not leave detached work mutating stale state.
- [ ] Add or retain deterministic/injectable clock seams where timing behavior is important to correctness.
- [ ] Stress the obvious race cases in tests: disconnect, reconnect, account switch, rapid sleep/wake retarget, settings changes, worker restart.

## 4. Cache policy and resource bounds

- [ ] Inventory every cache in the repo.
- [ ] For each cache, document or verify: source of truth, session scope, freshness state, TTL/retention, maximum entries/bytes, and invalidation event.
- [ ] Confirm policy/authorization state never derives from stale presentation-cache membership.
- [ ] Verify friend/profile metadata remains separate from whitelist/blacklist policy state.
- [ ] Confirm stale-but-usable presentation data cannot mask a user edit or account change.
- [ ] Verify repeated refresh failures cannot make entries immortal.
- [ ] Confirm image/thumbnail caches have their own appropriate size/retention bounds.
- [ ] Check pruning work is itself bounded and not performed as an expensive full scan on hot read paths.
- [ ] Expose useful cache size/age information through diagnostics where practical.
- [ ] Confirm there is no cache with an insertion path but no credible eviction/invalidation path.

## 5. Protocol and state model

- [ ] Review `AppState`, `StateEvent`, `WorkerReport`, commands, and capabilities for duplicated or obsolete fields.
- [ ] Ensure defaults are safe for older/missing serialized fields where backward compatibility is intentional.
- [ ] Remove protocol fields that became dead during prototype evolution.
- [ ] Confirm authoritative state and presentation/diagnostic state are visibly distinct.
- [ ] Verify disconnect semantics reset only the state that truly becomes invalid.
- [ ] Confirm worker reports are accepted only from the intended worker/client identity.
- [ ] Check protocol versioning still matches the actual compatibility contract.
- [ ] Make sure runtime-health and diagnostic fields do not accidentally become automation policy inputs.

## 6. VRChat hardening

- [ ] Audit every outbound VRChat endpoint through the request coordinator/rate-limit policy.
- [ ] Confirm `401`, `429`, transport failures, malformed responses, and operation-specific failures remain distinguishable where behavior differs.
- [ ] Verify login and 2FA rejection semantics are not incorrectly treated as ordinary expired-session errors.
- [ ] Preserve special evidence paths such as invite `403` refusal handling.
- [ ] Confirm redundant status/status-message writes are avoided.
- [ ] Verify capture -> modify -> restore behavior for temporary VRChat presence changes.
- [ ] Check current-user/session validation before actions that can affect another account.
- [ ] Confirm friend/profile fetches use stable VRChat user IDs as identity keys.
- [ ] Verify background refresh does not hammer VRChat after rate-limit or transport failures.
- [ ] Review all VRChat error strings/log fields for accidental cookie/token/body leakage.

## 7. UI, Taffy, rendering, and interaction

- [ ] Verify Taffy is the single geometry source for every migrated UI component.
- [ ] Search for leftover manual rectangles, offsets, widths/heights, or hit-test ledgers that duplicate Taffy output.
- [ ] Confirm rendering and hit-testing consume the same computed rectangles.
- [ ] Check nested/root-relative layout calculations and clipping behavior.
- [ ] Test representative resize/content-growth cases so text or dynamic rows cannot silently overlap controls.
- [ ] Verify COSMIC Text measurement/wrapping remains the text source of truth.
- [ ] Check focus/hover/press semantics for mouse/controller/VR laser paths.
- [ ] Preserve raw controller interaction behavior unless a deliberate replacement is documented.
- [ ] Verify visual effects do not change hit geometry or make disabled controls appear interactive.
- [ ] Check the Home / VR Settings / Desktop Settings responsibility boundary remains intact.
- [ ] Keep Hydra/easter-egg behavior intentional; remove only accidental debug leftovers, not personality.

## 8. OpenXR / VR runtime behavior

- [ ] Audit world-space transforms, panel placement, scale, handedness, and anchoring separately from 2D UI layout.
- [ ] Confirm no 2D layout engine state is being misused for hand/controller/world transforms.
- [ ] Verify overlay reconnect/restart behavior restores only intended remembered state.
- [ ] Check pose persistence and spawn-mode behavior for invalid/old data.
- [ ] Confirm lost tracking/disconnect paths cannot leave stale interaction state active.
- [ ] Test headset-friendly target sizes and primary controls at realistic panel scale.

## 9. HR and OSC automation

- [ ] Verify HR provider connection/search/error states transition coherently and clear stale device/BPM state where required.
- [ ] Check HR sleep detection cooldown, calm period, manual wake suppression, and repeated-event behavior.
- [ ] Confirm OSC avatar/parameter state resets correctly on worker/avatar disconnect.
- [ ] Verify GoGo/OSC automation does not depend on stale avatar metadata.
- [ ] Review outbound OSC behavior for duplicate spam and ensure automation writes are bounded.
- [ ] Test worker restarts without daemon restart.

## 10. Runtime health and diagnostics

- [ ] Confirm the runtime-health snapshot represents the useful operational truth without becoming a second application-state model.
- [ ] Check worker connectivity, last success, last failure, stale/degraded markers, and refresh age semantics.
- [ ] Verify health timestamps/errors update at centralized report/disconnect boundaries where possible.
- [ ] Confirm the bounded event ring has a hard capacity and deterministic eviction behavior.
- [ ] Ensure event messages are short, sanitized, and useful for diagnosing automation decisions.
- [ ] Verify diagnostics never collect credentials, session cookies, environment variables, process command lines, or unnecessary social identity data.
- [ ] Include cache size/age and runtime health in reports where useful.
- [ ] Confirm debug-report retention remains bounded and custom output paths are not unexpectedly pruned.
- [ ] Review process metrics labels for accuracy; do not label accumulated CPU runtime as live CPU percentage.
- [ ] Exercise a real Linux debug report and inspect the output manually once.

## 11. Error handling and logging

- [ ] Search for `unwrap`, `expect`, ignored `Result`, and silent fallbacks in runtime code; justify every remaining one.
- [ ] Search for string matching on errors where typed errors should drive behavior.
- [ ] Confirm expected recoverable failures do not produce panic-level behavior.
- [ ] Verify malformed/invalid authoritative data fails loudly rather than becoming a plausible default.
- [ ] Review log levels for noise: normal lifecycle info, recoverable warning, true error.
- [ ] Remove duplicate logging of the same failure at multiple layers unless each layer adds useful context.
- [ ] Check logs for secrets, auth tokens, notification bodies, user IDs/names where they are not diagnostically necessary.
- [ ] Keep useful funny comments, but ensure the code underneath them is still immediately understandable.

## 12. Tests and CI

- [ ] Run the complete CI workflow from the final audit candidate.
- [ ] Audit every `#[path = "../src/...rs"]` standalone integration harness against the final module graph.
- [ ] Make sure structural dependency changes are represented in all affected harnesses.
- [ ] Add/retain golden-path end-to-end coverage for the primary sleep -> automation -> wake path.
- [ ] Verify tests cover cancellation/generation, stale cache, policy-vs-presentation separation, rate limiting, and disconnect/reconnect behavior.
- [ ] Check negative tests for malformed protocol/API data and privacy redaction.
- [ ] Remove brittle tests that only encode implementation trivia unless that trivia is a contract.
- [ ] Confirm no test depends on live VRChat/network access unless explicitly isolated from normal CI.
- [ ] Run clippy/format/test gates with the same flags CI uses.
- [ ] Inspect the final CI logs for warnings that pass but indicate cleanup debt.

## 13. Performance and resource behavior

- [ ] Look for hot-loop allocations, repeated layout/text rebuilds, unnecessary clones of large state, and full-cache scans.
- [ ] Confirm background work is coarse/bounded enough for a sleep companion running for many hours.
- [ ] Check memory growth over a long-running representative session.
- [ ] Verify file-descriptor/task counts remain stable across worker reconnect cycles.
- [ ] Check retry loops have backoff/bounds and cannot become busy loops.
- [ ] Confirm image/thumbnail decode/download work is not duplicated unnecessarily.
- [ ] Review frame/update paths for accidental blocking filesystem, keyring, or network work.

## 14. Privacy and security

- [ ] Re-run the secret/privacy audit across logs, debug reports, panic messages, serialized state, and test fixtures.
- [ ] Confirm VRChat cookies/tokens are redacted in `Debug` implementations and error paths.
- [ ] Validate user IDs before path interpolation or other structured API use.
- [ ] Check file permissions/locations for state, diagnostics, and credentials.
- [ ] Ensure stale data from a previous VRChat account cannot appear after switching accounts.
- [ ] Confirm diagnostics and event history collect only what is necessary for Nightshift support/debugging.
- [ ] Review IPC trust boundaries and reject reports/commands from inappropriate client identities.

## 15. Dependencies, features, and build surface

- [ ] Review all Cargo dependencies for unused crates/features introduced during prototyping.
- [ ] Remove unused feature flags and dead optional paths.
- [ ] Check dependency versions for obvious duplicate major versions that can be collapsed safely.
- [ ] Confirm Taffy/COSMIC Text/OpenXR-related feature selections match actual usage.
- [ ] Inspect release/profile settings for accidental prototype-only debug behavior.
- [ ] Verify README/build instructions match the final workspace and required system dependencies.

## 16. Clipboard and goblin sweep

- [ ] Search for `TODO`, `FIXME`, `HACK`, `TEMP`, `XXX`, `delete-me`, `prototype-only`, and similar markers.
- [ ] Search for commented-out implementations and dead code kept "just in case".
- [ ] Search for duplicated literals/constants that should have one owner.
- [ ] Remove temporary debug UI, prints, one-off tracing fields, and abandoned experiment toggles.
- [ ] Check old compatibility aliases and transitional helper names that no longer describe reality.
- [ ] Look for copy/paste branches of logic that have already drifted from each other.
- [ ] Review humorous comments: keep the ones that help humans; evict goblins that obscure contracts or lie about behavior.
- [ ] Confirm no clipboard code remains simply because deleting it feels scary. Git remembers things for us.

## 17. Documentation and contributor contract

- [ ] Update architecture/roadmap docs to describe the finished prototype rather than the journey that produced it.
- [ ] Mark completed roadmap items clearly and move future/product ideas out of the prototype exit criteria.
- [ ] Ensure `CONTRIBUTING.md` reflects the final module/test/promotion workflow.
- [ ] Document cache lifecycle rules and authoritative-vs-presentation boundaries near the relevant architecture docs.
- [ ] Document the expected branch/promotion/CI process for the production-hardening phase.
- [ ] Make sure examples and screenshots do not show obsolete UI or settings.

## 18. Final canonical gate

- [ ] Re-fetch canonical immediately before the final audit promotion.
- [ ] Verify every audit-fix branch is either promoted, intentionally deferred with an issue, or discarded.
- [ ] Compare the final candidate against the pre-audit canonical and manually review the complete aggregate diff.
- [ ] Confirm all required CI is completed and green on the exact final candidate SHA.
- [ ] Run the primary user journey manually: launch -> connect workers -> VR interaction -> enter sleep -> automation -> wake -> restore -> reconnect/restart.
- [ ] Generate and inspect a final debug report from that run.
- [ ] Confirm no known correctness/privacy/data-loss issue remains hidden behind "prototype" status.
- [ ] Record any deliberately deferred non-blocking work as explicit post-prototype issues.
- [ ] Tag/record the final prototype-exit SHA.

## Prototype exit definition

The prototype phase is done when:

- [ ] the roadmap's prototype-scoped functionality is complete;
- [ ] canonical has one understandable source of truth for layout, policy, identity, freshness, and automation state;
- [ ] async/session/cache boundaries cannot leak stale work across identities or newer state;
- [ ] queues, caches, logs, diagnostics, and background work are bounded;
- [ ] the full test/CI gate is green;
- [ ] a complete diff and privacy audit are understood;
- [ ] obsolete branches, clipboard code, compatibility ledgers, and unsupervised goblins are either removed or have an explicit documented reason to remain.

Then `prototype/integrated-mvp` can become the baseline for production hardening instead of the place where unfinished experiments go to reproduce.
