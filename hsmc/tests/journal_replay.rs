//! Replay tests: a hand-built EXPECTED journal compared byte-for-byte against
//! the ACTUAL journal produced by running the chart with a fixed event
//! sequence. This is the canonical correctness check the user asked for —
//! "EVERYTHING in the sequence has to be journaled and compared against the
//! expected sequence."
//!
//! When the codegen drifts (an entry actions reorders, a timer arms in a
//! different place, an event dispatches to a different state), exactly one
//! line of the expected vector mismatches and the test fails with a
//! pinpoint diff.

#![cfg(all(feature = "tokio", feature = "journal"))]
#![allow(unexpected_cfgs)]

use hsmc::{statechart, ActionKind, TraceEvent};

#[derive(Default)]
pub struct Ctx;

#[derive(Debug, Clone, PartialEq)]
pub enum Ev {
    Go,
    Halt,
}

statechart! {
    Replay {
        context: Ctx;
        events:  Ev;

        default(Idle);
        terminate(Halt);

        entry: root_in;
        exit:  root_out;

        state Idle {
            entry: idle_in;
            exit:  idle_out;
            on(Go) => Active;
        }
        state Active {
            entry: active_in;
            exit:  active_out;
            default(Sub);
            state Sub {
                entry: sub_in;
                exit:  sub_out;
            }
        }
    }
}

impl ReplayActions for ReplayActionContext<'_> {
    async fn root_in(&mut self)    {}
    async fn root_out(&mut self)   {}
    async fn idle_in(&mut self)    {}
    async fn idle_out(&mut self)   {}
    async fn active_in(&mut self)  {}
    async fn active_out(&mut self) {}
    async fn sub_in(&mut self)     {}
    async fn sub_out(&mut self)    {}
}

// State indices.
const ST_ROOT: u16 = 0;
const ST_IDLE: u16 = 1;
const ST_ACTIVE: u16 = 2;
const ST_SUB: u16 = 3;

// Action indices.
const A_ROOT_IN: u16 = 0;
const A_ROOT_OUT: u16 = 1;
const A_IDLE_IN: u16 = 2;
const A_IDLE_OUT: u16 = 3;
const A_ACTIVE_IN: u16 = 4;
const A_ACTIVE_OUT: u16 = 5;
const A_SUB_IN: u16 = 6;
const A_SUB_OUT: u16 = 7;

// Event indices.
const E_GO: u16 = 0;
const E_HALT: u16 = 1;

#[tokio::test(flavor = "current_thread")]
async fn replay_full_lifecycle_matches_expected() {
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Replay::new(Ctx::default());
        let _ = m.dispatch(Ev::Go).await;
        let _ = m.dispatch(Ev::Halt).await;

        let actual = m.take_journal();

        let expected: Vec<TraceEvent> = vec![
            // ── First step: enter the chart ──
            TraceEvent::Started { chart_hash: Replay::<8>::CHART_HASH },
            TraceEvent::Entered { state: ST_ROOT },
            TraceEvent::ActionInvoked { state: ST_ROOT, action: A_ROOT_IN, kind: ActionKind::Entry },
            TraceEvent::Entered { state: ST_IDLE },
            TraceEvent::ActionInvoked { state: ST_IDLE, action: A_IDLE_IN, kind: ActionKind::Entry },

            // ── Go event delivered to Idle, transition to Active → Sub ──
            TraceEvent::EventDelivered { handler_state: ST_IDLE, event: E_GO },
            TraceEvent::TransitionFired { from: Some(ST_IDLE), to: ST_ACTIVE },
            TraceEvent::ActionInvoked { state: ST_IDLE, action: A_IDLE_OUT, kind: ActionKind::Exit },
            TraceEvent::Exited { state: ST_IDLE },
            TraceEvent::Entered { state: ST_ACTIVE },
            TraceEvent::ActionInvoked { state: ST_ACTIVE, action: A_ACTIVE_IN, kind: ActionKind::Entry },
            TraceEvent::Entered { state: ST_SUB },
            TraceEvent::ActionInvoked { state: ST_SUB, action: A_SUB_IN, kind: ActionKind::Entry },

            // ── Halt event triggers termination: exit Sub → Active → root, then Terminated ──
            TraceEvent::TerminateRequested { event: E_HALT },
            TraceEvent::ActionInvoked { state: ST_SUB, action: A_SUB_OUT, kind: ActionKind::Exit },
            TraceEvent::Exited { state: ST_SUB },
            TraceEvent::ActionInvoked { state: ST_ACTIVE, action: A_ACTIVE_OUT, kind: ActionKind::Exit },
            TraceEvent::Exited { state: ST_ACTIVE },
            TraceEvent::ActionInvoked { state: ST_ROOT, action: A_ROOT_OUT, kind: ActionKind::Exit },
            TraceEvent::Exited { state: ST_ROOT },
            TraceEvent::Terminated,
        ];

        let mismatch_at = actual.iter().zip(expected.iter())
            .position(|(a, e)| a != e)
            .map(|i| i.to_string())
            .unwrap_or_else(|| "<length-mismatch>".to_string());
        assert_eq!(actual, expected, "journal divergence at index {}", mismatch_at);
    }).await;
}
