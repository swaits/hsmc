//! Deterministic instruction-count benches for the dispatch hot path.
//!
//! Run with `cargo bench --bench dispatch` (Linux only — uses
//! iai-callgrind + valgrind).
//!
//! Each bench measures one dispatch kind in isolation:
//!  - `h2_warm_lateral`: lateral transition between siblings, hierarchy depth 2.
//!  - `h4_cross_tree`: lateral transition crossing two parallel subtrees, depth 4 from root.
//!  - `h8_cross_tree`: same shape but depth 8 — exercises LCA worst-case.
//!  - `h4_up`: up-transition (target is an ancestor of current).
//!  - `h4_self`: self-transition (target == current).
//!  - `h4_drop`: send an event with no matching handler (drop path).
//!  - `h4_self_emit_chain`: action emits next event 3×; tests the dispatch drain loop.
//!  - `h2_cold_start`: first dispatch into a freshly-constructed chart (priming step).
//!
//! Each bench's setup function primes the chart to a known state outside the
//! measured region. The body sends one event and runs `step()` once.

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("hsmc dispatch benches require Linux (iai-callgrind + valgrind).");
}

#[cfg(target_os = "linux")]
use iai_callgrind::{library_benchmark, library_benchmark_group, main};
#[cfg(target_os = "linux")]
use std::hint::black_box;
#[cfg(target_os = "linux")]
use hsmc::{statechart, Duration};

// ──────────────────────────────────────────────────────────────────────
// Chart 1: H=2 sibling. LCA(A, B) = P. Two states deep.
// ──────────────────────────────────────────────────────────────────────
#[cfg(target_os = "linux")]
mod h2 {
    use super::*;

    #[derive(Default)]
    pub struct Ctx;

    #[derive(Debug, Clone)]
    pub enum Ev {
        Go,
    }

    statechart! {
        H2 {
            context: Ctx;
            events:  Ev;
            default(P);

            state P {
                default(A);
                state A { on(Go) => B; }
                state B { entry: noop; }
            }
        }
    }

    impl H2Actions for H2ActionContext<'_> {
        fn noop(&mut self) {}
    }
}

// ──────────────────────────────────────────────────────────────────────
// Chart 2: H=4 cross-tree. LCA(L3, R3) = root. Stresses LCA over 8 ancestors.
// Also has up- and self-transition events for additional benches.
// ──────────────────────────────────────────────────────────────────────
#[cfg(target_os = "linux")]
mod h4 {
    use super::*;

    #[derive(Default)]
    pub struct Ctx;

    #[derive(Debug, Clone)]
    pub enum Ev {
        Cross,
        Up,
        Self_,
    }

    statechart! {
        H4 {
            context: Ctx;
            events:  Ev;
            default(L1);

            state L1 {
                default(L2);
                state L2 {
                    default(L3);
                    state L3 {
                        on(Cross) => R3;
                        on(Up)    => L1;
                        on(Self_) => L3;
                    }
                }
            }
            state R1 {
                default(R2);
                state R2 {
                    default(R3);
                    state R3 { entry: noop; }
                }
            }
        }
    }

    impl H4Actions for H4ActionContext<'_> {
        fn noop(&mut self) {}
    }
}

// ──────────────────────────────────────────────────────────────────────
// Chart 3: H=8 cross-tree. LCA(L7, R7) = root. Worst-case for the
// pre-fix O(H²) LCA — old code does ~64 compares; new code does ~16.
// ──────────────────────────────────────────────────────────────────────
#[cfg(target_os = "linux")]
mod h8 {
    use super::*;

    #[derive(Default)]
    pub struct Ctx;

    #[derive(Debug, Clone)]
    pub enum Ev {
        Cross,
    }

    statechart! {
        H8 {
            context: Ctx;
            events:  Ev;
            default(L1);

            state L1 { default(L2);
                state L2 { default(L3);
                    state L3 { default(L4);
                        state L4 { default(L5);
                            state L5 { default(L6);
                                state L6 { default(L7);
                                    state L7 { on(Cross) => R7; }
                                }
                            }
                        }
                    }
                }
            }
            state R1 { default(R2);
                state R2 { default(R3);
                    state R3 { default(R4);
                        state R4 { default(R5);
                            state R5 { default(R6);
                                state R6 { default(R7);
                                    state R7 { entry: noop; }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    impl H8Actions for H8ActionContext<'_> {
        fn noop(&mut self) {}
    }
}

// ──────────────────────────────────────────────────────────────────────
// Chart 4: emit-chain. S0 →(Go)→ S1 →(emits Next)→ S2 →(emits Next)→ S3 →(emits Next)→ S4.
// One Send + one Step ⇒ chart drains 4 events through 3 self-emits.
// ──────────────────────────────────────────────────────────────────────
#[cfg(target_os = "linux")]
mod chain {
    use super::*;

    #[derive(Default)]
    pub struct Ctx;

    #[derive(Debug, Clone)]
    pub enum Ev {
        Go,
        Next,
    }

    statechart! {
        Chain {
            context: Ctx;
            events:  Ev;
            default(S0);

            state S0 { on(Go) => S1; }
            state S1 { entry: emit_next; on(Next) => S2; }
            state S2 { entry: emit_next; on(Next) => S3; }
            state S3 { entry: emit_next; on(Next) => S4; }
            state S4 { entry: settle; }
        }
    }

    impl ChainActions for ChainActionContext<'_> {
        fn emit_next(&mut self) {
            let _ = self.emit(Ev::Next);
        }
        fn settle(&mut self) {}
    }
}

// ──────────────────────────────────────────────────────────────────────
// Setup helpers. Prime each chart to its measurement starting state
// OUTSIDE the measured region — iai-callgrind only counts work inside
// the bench fn body.
// ──────────────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
fn setup_h2_warm() -> h2::H2<8> {
    let mut m = h2::H2::new(h2::Ctx::default());
    let _ = m.step(Duration::ZERO); // priming step → enters P → A
    m
}

#[cfg(target_os = "linux")]
fn setup_h2_cold() -> h2::H2<8> {
    h2::H2::new(h2::Ctx::default())
}

#[cfg(target_os = "linux")]
fn setup_h4_warm() -> h4::H4<8> {
    let mut m = h4::H4::new(h4::Ctx::default());
    let _ = m.step(Duration::ZERO); // priming step → L1 → L2 → L3
    m
}

#[cfg(target_os = "linux")]
fn setup_h8_warm() -> h8::H8<8> {
    let mut m = h8::H8::new(h8::Ctx::default());
    let _ = m.step(Duration::ZERO); // priming step → L1..L7
    m
}

#[cfg(target_os = "linux")]
fn setup_chain_warm() -> chain::Chain<8> {
    let mut m = chain::Chain::new(chain::Ctx::default());
    let _ = m.step(Duration::ZERO); // priming step → S0
    m
}

// ──────────────────────────────────────────────────────────────────────
// Benches. One library_benchmark per scenario. Each takes its primed
// chart by value, sends one event, and runs one step.
// ──────────────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
#[library_benchmark]
#[bench::cold(setup = setup_h2_cold)]
fn h2_cold_start(mut m: h2::H2<8>) -> h2::H2<8> {
    // first .step() primes the chart by entering the default state path
    let _ = black_box(m.step(black_box(Duration::ZERO)));
    m
}

#[cfg(target_os = "linux")]
#[library_benchmark]
#[bench::warm(setup = setup_h2_warm)]
fn h2_warm_lateral(mut m: h2::H2<8>) -> h2::H2<8> {
    let _ = black_box(m.send(black_box(h2::Ev::Go)));
    let _ = black_box(m.step(black_box(Duration::ZERO)));
    m
}

#[cfg(target_os = "linux")]
#[library_benchmark]
#[bench::cross(setup = setup_h4_warm)]
fn h4_cross_tree(mut m: h4::H4<8>) -> h4::H4<8> {
    let _ = black_box(m.send(black_box(h4::Ev::Cross)));
    let _ = black_box(m.step(black_box(Duration::ZERO)));
    m
}

#[cfg(target_os = "linux")]
#[library_benchmark]
#[bench::up(setup = setup_h4_warm)]
fn h4_up(mut m: h4::H4<8>) -> h4::H4<8> {
    let _ = black_box(m.send(black_box(h4::Ev::Up)));
    let _ = black_box(m.step(black_box(Duration::ZERO)));
    m
}

#[cfg(target_os = "linux")]
#[library_benchmark]
#[bench::self_(setup = setup_h4_warm)]
fn h4_self(mut m: h4::H4<8>) -> h4::H4<8> {
    let _ = black_box(m.send(black_box(h4::Ev::Self_)));
    let _ = black_box(m.step(black_box(Duration::ZERO)));
    m
}

#[cfg(target_os = "linux")]
#[library_benchmark]
#[bench::cross(setup = setup_h8_warm)]
fn h8_cross_tree(mut m: h8::H8<8>) -> h8::H8<8> {
    let _ = black_box(m.send(black_box(h8::Ev::Cross)));
    let _ = black_box(m.step(black_box(Duration::ZERO)));
    m
}

#[cfg(target_os = "linux")]
#[library_benchmark]
#[bench::chain(setup = setup_chain_warm)]
fn chain_self_emit(mut m: chain::Chain<8>) -> chain::Chain<8> {
    let _ = black_box(m.send(black_box(chain::Ev::Go)));
    // Drain the chain: 1 step per event. Go → S1 (entry emits Next) → S2
    // (entry emits Next) → S3 (entry emits Next) → S4. 4 events total.
    for _ in 0..4 {
        let _ = black_box(m.step(black_box(Duration::ZERO)));
    }
    m
}

#[cfg(target_os = "linux")]
library_benchmark_group!(
    name = dispatch;
    benchmarks =
        h2_cold_start,
        h2_warm_lateral,
        h4_cross_tree,
        h4_up,
        h4_self,
        h8_cross_tree,
        chain_self_emit,
);

#[cfg(target_os = "linux")]
main!(library_benchmark_groups = dispatch);
