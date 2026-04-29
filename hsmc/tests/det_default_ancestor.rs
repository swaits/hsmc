#![allow(dead_code)]
//! Deterministic-flow tests: `default(...)` targeting an ANCESTOR.
//! Inner's `default(Outer)` is a valid up-default. Inner is reachable
//! only via an explicit transition. When entered, its default fires
//! immediately, exiting Inner back to Outer (up-transition rule:
//! Outer is not re-entered).

#![cfg(all(feature = "tokio", feature = "journal"))]
#![allow(unexpected_cfgs)]

use hsmc::{statechart, TraceEvent};

#[derive(Default)]
pub struct Ctx;

#[derive(Debug, Clone)]
pub enum Ev {
    GoIn,
    Halt,
}

statechart! {
    AncestorDefault {
        context: Ctx;
        events:  Ev;
        default(Outer);
        terminate(Halt);

        state Outer {
            entry: outer_in;
            on(GoIn) => Inner;
            state Inner {
                entry: inner_in;
                exit:  inner_out;
                // Inner's default redirects up to Outer. After entering
                // Inner, this default fires as an up-transition that
                // exits Inner. Outer is not re-entered.
                default(Outer);
            }
        }
    }
}

impl AncestorDefaultActions for AncestorDefaultActionContext<'_> {
    async fn outer_in(&mut self) {}
    async fn inner_in(&mut self) {}
    async fn inner_out(&mut self) {}
}

const AD_ROOT: u16 = 0;
const AD_OUTER: u16 = 1;
const AD_INNER: u16 = 2;

#[tokio::test(flavor = "current_thread")]
async fn det_initial_lands_at_outer_no_default_descent() {
    // Outer has no default of its own — initial entry stops at Outer.
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = AncestorDefault::new(Ctx);
            let _ = m.step(hsmc::Duration::ZERO).await;
            assert_eq!(m.current_state(), AncestorDefaultState::Outer);
            let j = m.take_journal();
            let entered: Vec<u16> = j
                .iter()
                .filter_map(|e| match e {
                    TraceEvent::Entered { state } => Some(*state),
                    _ => None,
                })
                .collect();
            assert_eq!(entered, vec![AD_ROOT, AD_OUTER]);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_explicit_into_inner_bounces_back_via_up_default() {
    // `=> Inner` enters Inner (entries fire), then Inner's default fires
    // as an up-transition Inner → Outer. Outer is NOT re-entered (it
    // was already on the active path).
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = AncestorDefault::new(Ctx);
            let _ = m.dispatch(Ev::GoIn).await;

            assert_eq!(
                m.current_state(),
                AncestorDefaultState::Outer,
                "Inner's default bounces us back to Outer"
            );

            let j = m.take_journal();

            // Inner was entered (its entries fired) and then exited
            // (its default fired an up-transition).
            let entered_states: Vec<u16> = j
                .iter()
                .filter_map(|e| match e {
                    TraceEvent::Entered { state } => Some(*state),
                    _ => None,
                })
                .collect();
            // Across the full journal: Root, Outer, Inner. After Inner's
            // up-default to Outer, Outer is NOT re-entered.
            assert_eq!(entered_states, vec![AD_ROOT, AD_OUTER, AD_INNER]);

            let exited_states: Vec<u16> = j
                .iter()
                .filter_map(|e| match e {
                    TraceEvent::Exited { state } => Some(*state),
                    _ => None,
                })
                .collect();
            assert_eq!(
                exited_states,
                vec![AD_INNER],
                "only Inner exits; Outer remains on the active path"
            );

            // Outer's entry actions ran exactly once (initial entry only).
            let outer_entries = j
                .iter()
                .filter(|e| {
                    matches!(
                        e,
                        TraceEvent::ActionInvoked {
                            state: AD_OUTER,
                            kind: hsmc::ActionKind::Entry,
                            ..
                        }
                    )
                })
                .count();
            assert_eq!(outer_entries, 1, "Outer is entered once and stays put");
        })
        .await;
}
