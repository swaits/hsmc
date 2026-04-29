#![allow(dead_code)]
//! Deterministic-flow tests: `default(...)` targeting a SIBLING fires as
//! a real LCA-aware transition immediately after the declaring state's
//! entries finish. Foyer's `default(LivingRoom)` means: enter Foyer,
//! run its entries, then immediately transition Foyer → LivingRoom
//! (LCA = Root, exits Foyer, enters LivingRoom).

#![cfg(all(feature = "tokio", feature = "journal"))]
#![allow(unexpected_cfgs)]

use hsmc::{statechart, ActionKind, TraceEvent};

#[derive(Default)]
pub struct Ctx;

#[derive(Debug, Clone)]
pub enum Ev {
    Halt,
}

statechart! {
    SiblingDefault {
        context: Ctx;
        events:  Ev;
        default(Foyer);
        terminate(Halt);

        state Foyer {
            entry: foyer_in;
            exit:  foyer_out;
            default(LivingRoom);
        }
        state LivingRoom {
            entry: living_in;
            exit:  living_out;
        }
    }
}

impl SiblingDefaultActions for SiblingDefaultActionContext<'_> {
    async fn foyer_in(&mut self) {}
    async fn foyer_out(&mut self) {}
    async fn living_in(&mut self) {}
    async fn living_out(&mut self) {}
}

const SD_ROOT: u16 = 0;
const SD_FOYER: u16 = 1;
const SD_LIVING: u16 = 2;

fn entered(j: &[TraceEvent]) -> Vec<u16> {
    j.iter()
        .filter_map(|e| match e {
            TraceEvent::Entered { state } => Some(*state),
            _ => None,
        })
        .collect()
}

fn exited(j: &[TraceEvent]) -> Vec<u16> {
    j.iter()
        .filter_map(|e| match e {
            TraceEvent::Exited { state } => Some(*state),
            _ => None,
        })
        .collect()
}

#[tokio::test(flavor = "current_thread")]
async fn det_sibling_default_exits_declarer_lands_at_target() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = SiblingDefault::new(Ctx);
            let _ = m.step(hsmc::Duration::ZERO).await;

            assert_eq!(m.current_state(), SiblingDefaultState::LivingRoom);

            let j = m.take_journal();
            assert_eq!(
                entered(&j),
                vec![SD_ROOT, SD_FOYER, SD_LIVING],
                "Root → Foyer → (default fires) → LivingRoom"
            );
            assert_eq!(
                exited(&j),
                vec![SD_FOYER],
                "Foyer is exited as the default transition's `from` state"
            );

            // Foyer's entry must run before its exit (default fires after entries).
            let action_seq: Vec<(u16, ActionKind)> = j
                .iter()
                .filter_map(|e| match e {
                    TraceEvent::ActionInvoked { state, kind, .. } => Some((*state, *kind)),
                    _ => None,
                })
                .collect();
            let foyer_entry_idx = action_seq
                .iter()
                .position(|(s, k)| *s == SD_FOYER && *k == ActionKind::Entry)
                .unwrap();
            let foyer_exit_idx = action_seq
                .iter()
                .position(|(s, k)| *s == SD_FOYER && *k == ActionKind::Exit)
                .unwrap();
            let living_entry_idx = action_seq
                .iter()
                .position(|(s, k)| *s == SD_LIVING && *k == ActionKind::Entry)
                .unwrap();
            assert!(foyer_entry_idx < foyer_exit_idx);
            assert!(foyer_exit_idx < living_entry_idx);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_sibling_default_emits_transition_observation() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = SiblingDefault::new(Ctx);
            let _ = m.step(hsmc::Duration::ZERO).await;
            let j = m.take_journal();
            let txn_count = j
                .iter()
                .filter(|e| matches!(e, TraceEvent::TransitionFired { .. }))
                .count();
            assert!(
                txn_count >= 1,
                "expected at least one TransitionFired for the default-as-transition firing"
            );
        })
        .await;
}
