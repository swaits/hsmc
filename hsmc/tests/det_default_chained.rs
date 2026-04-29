#![allow(dead_code)]
//! Deterministic-flow tests: `default(...)` chains. Hub → Spoke1 →
//! Spoke2. Each link fires as a real LCA-aware transition. The
//! compile-time default-graph cycle check guarantees this terminates.

#![cfg(all(feature = "tokio", feature = "journal"))]
#![allow(unexpected_cfgs)]

use hsmc::{statechart, TraceEvent};

#[derive(Default)]
pub struct Ctx;

#[derive(Debug, Clone)]
pub enum Ev {
    Halt,
}

statechart! {
    ChainedDefault {
        context: Ctx;
        events:  Ev;
        default(Hub);
        terminate(Halt);

        state Hub {
            entry: hub_in;
            default(Spoke1);
        }
        state Spoke1 {
            entry: s1_in;
            exit:  s1_out;
            default(Spoke2);
        }
        state Spoke2 {
            entry: s2_in;
        }
    }
}

impl ChainedDefaultActions for ChainedDefaultActionContext<'_> {
    async fn hub_in(&mut self) {}
    async fn s1_in(&mut self) {}
    async fn s1_out(&mut self) {}
    async fn s2_in(&mut self) {}
}

const CD_ROOT: u16 = 0;
const CD_HUB: u16 = 1;
const CD_SPOKE1: u16 = 2;
const CD_SPOKE2: u16 = 3;

#[tokio::test(flavor = "current_thread")]
async fn det_chained_defaults_terminate_at_chain_end() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = ChainedDefault::new(Ctx);
            let _ = m.step(hsmc::Duration::ZERO).await;
            assert_eq!(m.current_state(), ChainedDefaultState::Spoke2);

            let j = m.take_journal();
            let entered: Vec<u16> = j
                .iter()
                .filter_map(|e| match e {
                    TraceEvent::Entered { state } => Some(*state),
                    _ => None,
                })
                .collect();
            assert_eq!(entered, vec![CD_ROOT, CD_HUB, CD_SPOKE1, CD_SPOKE2]);

            let exited: Vec<u16> = j
                .iter()
                .filter_map(|e| match e {
                    TraceEvent::Exited { state } => Some(*state),
                    _ => None,
                })
                .collect();
            // Each default fires as a sibling-transition: Hub → Spoke1
            // (exits Hub), then Spoke1 → Spoke2 (exits Spoke1).
            assert_eq!(exited, vec![CD_HUB, CD_SPOKE1]);

            // At least 2 TransitionFired events for the chained defaults.
            let txn_count = j
                .iter()
                .filter(|e| matches!(e, TraceEvent::TransitionFired { .. }))
                .count();
            assert!(txn_count >= 2);
        })
        .await;
}
