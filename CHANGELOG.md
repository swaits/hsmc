# Changelog

All notable changes to this workspace are documented here. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
this project adheres to [Semantic Versioning](https://semver.org/).

## 0.3.0 — 2026-04-27

### Added

- **`journal` Cargo feature.** Records every observable atom of chart
  execution — `Started`, `Entered`, `Exited`, `ActionInvoked`
  (`Entry`/`Exit`/`Handler`), `DuringStarted`, `DuringCancelled`,
  `TransitionFired`, `EventDelivered`, `EventDropped`, `EmitQueued`,
  `EmitFailed`, `TimerArmed`, `TimerCancelled`, `TimerFired`,
  `TerminateRequested`, `Terminated` — into a `Vec<TraceEvent>` on the
  generated machine. Same chart + same events = byte-identical
  journal. Public API: `journal()`, `take_journal()`, `clear_journal()`,
  `CHART_HASH` (a stable FNV-1a fingerprint of the chart's structural
  definition).
- **Root state targetable by chart name.** `on(Trig) => MyChart;`
  resolves to the root state in both `classify_target` and
  `resolve_transitions`. From any nested state this is an
  up-transition (root is always on the active path).
- **113 deterministic-flow tests** (under `--features tokio,journal`)
  comparing hand-built expected journals byte-for-byte against
  actual, anchored to spec sections: initial entry, lateral / self /
  up transitions, default-descent, event bubbling, timer arm /
  cancel / fire, emit, ordering, durings, step-vs-dispatch
  equivalence, root-targeting, `current_state()`, termination.
- **`verification/` workspace member with Creusot proofs.** Verified
  mirrors of the runtime `EventQueue` and `TimerTable` carry
  `creusot-std` (Pearlite) contracts that Why3 + alt-ergo + z3
  discharge mechanically. 14/14 VCs proven. `just verify` is
  self-installing (mise pulls opam + the pinned nightly + Creusot's
  canonical `INSTALL`).
- **`docs/002. hsmc-semantics-formal.md`** — canonical, unambiguous
  reference for chart behavior. Numbered S-rules supersede the v0.1
  prose spec and v0.2 design-change-request.
- **README "How a chart behaves" section** — eight building-block
  rules that build on each other, with an explicit "what hsmc
  deliberately does NOT have" table.

### Tooling

- `mise.toml` (root) declares stable Rust + components + embedded
  targets + cargo subcommands. `verification/mise.toml` declares
  the pinned `nightly-2025-11-13` + opam + python venv + uv.
- `set shell` and `set script-interpreter` route every `just` recipe
  through `mise exec` — no shebangs, no per-recipe wrappers.
- `just verify` runs `cargo creusot prove --no-cache` so SMT
  invocations happen on every run; `time` shows ~7s CPU on ~1.2s
  wall = real parallel work.
- `just mutants` runs with `--features tokio,journal` so `journal_*`
  and `det_*` tests execute. **0 missed mutants.**
- `just miri` excludes `hsmc-macros` (trybuild glob hits Miri's
  isolation) and `hsmc-verification` (different pinned toolchain).
  66 tests pass under Miri, 0 UB.

### Backward compatibility

Every v0.1 / v0.2 statechart compiles under v0.3 unchanged.

## 0.2.0 — 2026-04-22

### Fixed — behavior-visible (breaking)

- **Up-transitions no longer exit and re-enter the target ancestor.**
  A transition whose target is already active (i.e., the target is on
  the path from root to the innermost active state, and the target is
  not itself the innermost) now unwinds only the subtree strictly
  below the target. The target's `exit:` / `entry:` actions do NOT
  fire, and its `default(...)` does NOT re-descend. You cannot
  re-enter a state you never left. The old semantics (exit target,
  re-enter, re-descend to default) was incorrect and could produce
  infinite re-entry when a child transitioned to its own parent.
  See spec §2.6, §T2.8, §T2.8b, §T2.8c.
- Self-transitions (`target == innermost`) are unchanged: the target
  is still exited and re-entered, timers still restart.
- Lateral transitions (target not on the active path) are unchanged.
- `current_state()` may now return a composite state when it became
  innermost via an up-transition.

## 0.1.0 — 2026-04-22

Initial release.

### Added

- `statechart!` proc macro: declarative Harel-style hierarchical state
  machines with nested states, `entry:` / `exit:` actions, LCA-aware
  transitions, default-child descent, and `terminate` semantics.
- Typed event payload bindings:
  `on(PacketRx { rssi: i16 }) => handler;` expands into a typed handler
  signature, verified at compile time.
- Timer triggers: `on(after Duration::from_secs(5)) => Next;` and
  `on(every Duration::from_millis(100)) => tick;`, tracked in a
  fixed-capacity `TimerTable`.
- `during:` activities (v0.2 semantics): async functions that run
  while a state is on the active path and produce events. Multiple
  concurrent durings on a single state borrow disjoint `&mut` slices of
  the context, enforced by Rust's native split-borrow.
- Runtime: `machine.run().await` races active durings, the external
  event channel, and timer deadlines — one future, cooperative on
  embassy, multi-threaded on tokio.
- Feature gates: `tokio` (std + tokio runtime), `embassy` (no_std +
  embassy-time / embassy-sync). The two features are mutually
  exclusive and enforced via `compile_error!`.
- `no_std` by default; `std` pulled in only via the `tokio` feature.
- `STATE_CHART` const and `dispatch()` helper for introspection.
- Workspace: `hsmc` (library) and `hsmc-macros` (proc-macro front-end)
  both at `0.1.0`.

### Docs

- 320-line `README.md` with quickstart, semantics, and examples.
- Runnable examples: `microwave` and `during_radio` under tokio,
  `embassy_full` under embassy.
- Behavior, hierarchy, during, timer, and payload-binding test suites
  (13 integration tests).
