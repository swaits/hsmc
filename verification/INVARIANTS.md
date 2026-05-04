# Invariants Mapping

Maps spec rules from `docs/002. hsmc-semantics-formal.md` to Pearlite
contracts in this crate. Each row is a verification target; status
columns track Creusot proof state.

## Status legend

- ✅  proof discharged by Creusot
- 🟡  contract written, proof not yet attempted (Creusot not installed)
- 🔴  contract incomplete or proof failing
- ⏭  out of scope for this phase

## EventQueue (`src/event_queue.rs`)

| Spec rule | Statement | Function | Status |
|-----------|-----------|----------|--------|
| S6.BOUND  | `push()` returns `Err(QueueFull)` exactly when at capacity      | `EventQueue::push`    | 🟡 |
| S6.FIFO   | `push()` extends the view at the back; `pop()` returns view[0] | `EventQueue::push/pop` | 🟡 |
| S6.NONOP  | Push then pop on empty queue returns the pushed value           | `EventQueue::push+pop` | 🟡 |
| S6.EMPTY  | `is_empty()` ⇔ `view().len() == 0`                              | `EventQueue::is_empty` | 🟡 |
| S6.CLEAR  | `clear()` makes the view empty                                  | `EventQueue::clear`    | 🟡 |
| S6.LEN    | `len()` returns the view length                                  | `EventQueue::len`      | 🟡 |

## Path tables (`src/paths.rs`)

The codegen-emitted tables `__PATH_RANGE`, `__PATH_DATA`,
`__TERMINATE_RANGE`, `__TERMINATE_DATA`, and `__DEFAULT_CHILD` carry
the safety preconditions for every `unsafe { ... .get_unchecked(...) }`
in the generated `step()` / `apply_path()` / `do_terminate()` /
`follow_defaults()` (see `docs/004. unsafe-safety-contract.md`). The
proof strategy is two-pronged:

- A `const _: () = { assert!(...) };` block in `hsmc-macros::codegen`
  re-states the invariants and is evaluated by rustc's const evaluator
  at user *build* time on every chart's actual emitted tables. Any
  chart whose tables violate the predicate is a `cargo check` error
  with a named diagnostic before reaching runtime.
- The Pearlite-annotated functions in `paths.rs` express the *same
  predicate* with an `#[ensures(... forall ...)]` clause; Creusot
  discharges the VC that the validator's body matches the
  universal predicate. `tests/correspondence_paths.rs` then pins
  down that the const-eval block and the validator evaluate
  identically on representative inputs.

| Spec rule | Statement | Function | Status |
|-----------|-----------|----------|--------|
| P1.PATH      | `forall i. let (s,m,e) = __PATH_RANGE[i] in s ≤ m ≤ e ≤ __PATH_DATA.len()`         | `check_path_range_invariant`     | 🟡 |
| P1.TERM      | `forall i. let (s,e) = __TERMINATE_RANGE[i] in s ≤ e ≤ __TERMINATE_DATA.len()`     | `check_terminate_range_invariant` | 🟡 |
| P2.PATH_ID   | `forall i. __PATH_DATA[i] < n_states`                                              | `check_state_ids_in_bounds`       | 🟡 |
| P2.TERM_ID   | `forall i. __TERMINATE_DATA[i] < n_states`                                         | `check_state_ids_in_bounds`       | 🟡 |
| P2.DEFAULT   | `forall i. match __DEFAULT_CHILD[i] { Some(t) => t < n_states, None => () }`       | `check_default_child_in_bounds`   | 🟡 |
| P.AGGREGATE  | All five above hold simultaneously                                                  | `check_path_tables_sound`         | 🟡 |

Each function's `#[ensures]` clause is a `forall<i: Int>` over the
input slice. Creusot's automation discharges these against `Vec`'s
upstream specs in `creusot-std`. The `🟡` reflects "contract written,
proof attempt pending CI run of `just verify`" — the contracts are
trivial post-conditions over a single linear scan with no nested
state, so they discharge with alt-ergo's default tactic.

## TimerTable (`src/timer_table.rs`)

| Spec rule    | Statement | Function | Status |
|--------------|-----------|----------|--------|
| S5.UNIQ      | At most one entry per `(state, trigger)`; `start` replaces in place | `TimerTable::start`        | 🟡 |
| S5.CAP       | View length ≤ CAP across all operations                            | (all)                       | 🟡 |
| S5.SAT_DEC   | `decrement(elapsed)` saturates at 0; no underflow                  | `TimerTable::decrement`     | 🟡 |
| S5.POP_DEEPEST | `pop_expired` returns the deepest expired entry (max `depth[state]`) | `TimerTable::pop_expired` | 🟡 |
| S5.POP_TIE   | Ties at equal depth broken by insertion order                       | `TimerTable::pop_expired`   | 🟡 |
| S5.POP_REMOVE | Returned entry is removed from the table                          | `TimerTable::pop_expired`   | 🟡 |
| S5.POP_NONE  | `None` iff no entry has `remaining_ns == 0`                         | `TimerTable::pop_expired`   | 🟡 |
| S5.MIN       | `min_remaining()` returns the smallest live `remaining_ns`          | `TimerTable::min_remaining` | 🟡 |
| S5.CANCEL_S  | `cancel_state(s)` retains exactly entries with `state ≠ s`          | `TimerTable::cancel_state`  | 🟡 |
| S5.CANCEL_1  | `cancel_one(s, t)` retains exactly entries `≠ (s, t)`               | `TimerTable::cancel_one`    | 🟡 |

## Out of scope (this phase)

| Item | Reason |
|------|--------|
| `compute_transition_paths` algorithm itself              | Universal proof "for any tree input, output satisfies P1 + P2" requires modeling the tree structure in Pearlite. The current strategy proves the *predicate* is correct and runs it via const-eval on every actual chart at build time — operationally equivalent for end users (every compiled chart is verified) but not the universal quantification that Creusot would give. Phase 5. |
| `step()` end-to-end determinism                           | Composes everything above; do once helpers are green. Phase 5. |
| Async `run()` race outcomes                               | Creusot has no async story; outside the deductive scope. |
| User action bodies                                        | User code, not part of the runtime. |
| `duration_from_secs_f64`                                  | Floating-point not modeled. Use `#[trusted]` + property tests. |

## How to verify

```bash
just verify
```

That's it. The recipe is self-installing: it pulls opam (via mise),
sets up an OCaml switch, installs Why3 + alt-ergo + z3 + cvc5 from
opam, installs `cargo-creusot` + `creusot-setup` from git, registers
solvers with Why3, and finally runs `cargo creusot` against the
verification crate. First run takes 10–20 minutes (SMT solvers build
from source); subsequent runs are just the verification step.

Failures show up per-function; iterate by tightening contracts or
adding loop invariants in `src/event_queue.rs` / `src/timer_table.rs`.

## Correspondence with the runtime

The mirrors in `src/event_queue.rs` and `src/timer_table.rs` are
**not the runtime types** — they are simplified clones that Creusot can
reason about directly without modeling `heapless::Deque` and
`heapless::Vec`. They are kept observably equivalent to
`hsmc::__private::EventQueue` and `hsmc::__private::TimerTable` by:

- `tests/correspondence_event_queue.rs`
- `tests/correspondence_timer_table.rs`

These run both the mirror and the runtime type with identical operation
sequences and assert their visible behavior agrees on every step. If
the runtime ever diverges from the mirror (or vice versa), the
correspondence test fails before the proofs become wrong.
