# Changelog

All notable changes to this workspace are documented here. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
this project adheres to [Semantic Versioning](https://semver.org/).

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
