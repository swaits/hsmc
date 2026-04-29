#![allow(dead_code)]
//! Deterministic-flow tests for OPTIONAL `default(...)` on composite states.
//!
//! Spec: a composite state may declare `default(...)` or omit it. When
//! omitted, transitions targeting the composite land on the composite itself
//! — no default-descent. Substates are reachable only via explicit
//! transitions. This is the "microwave" pattern: a `Standby` state with
//! sub-modes (e.g. `LowPower`) that the chart enters only on demand.

#![cfg(all(feature = "tokio", feature = "journal"))]
#![allow(unexpected_cfgs)]

use hsmc::{statechart, ActionKind, TraceEvent};

#[derive(Default)]
pub struct Ctx;

#[derive(Debug, Clone)]
pub enum Ev {
    PowerOn,
    PowerOff,
    EnterLowPower,
    LeaveLowPower,
    Halt,
}

statechart! {
    Microwave {
        context: Ctx;
        events:  Ev;
        default(Standby);
        terminate(Halt);

        state Standby {
            // No `default(...)` — Standby itself is the resting state.
            entry: standby_in;
            exit:  standby_out;
            on(PowerOn) => On;
            on(EnterLowPower) => LowPower;

            state LowPower {
                entry: low_in;
                exit:  low_out;
                on(LeaveLowPower) => Standby;
            }
        }

        state On {
            entry: on_in;
            exit:  on_out;
            on(PowerOff) => Standby;
        }
    }
}

impl MicrowaveActions for MicrowaveActionContext<'_> {
    async fn standby_in(&mut self) {}
    async fn standby_out(&mut self) {}
    async fn low_in(&mut self) {}
    async fn low_out(&mut self) {}
    async fn on_in(&mut self) {}
    async fn on_out(&mut self) {}
}

// State ids: 0=Root, 1=Standby, 2=LowPower, 3=On.
const SR: u16 = 0;
const S_STANDBY: u16 = 1;
const S_LOW: u16 = 2;
const S_ON: u16 = 3;

fn entered_states(j: &[TraceEvent]) -> Vec<u16> {
    j.iter()
        .filter_map(|e| match e {
            TraceEvent::Entered { state } => Some(*state),
            _ => None,
        })
        .collect()
}

fn exited_states(j: &[TraceEvent]) -> Vec<u16> {
    j.iter()
        .filter_map(|e| match e {
            TraceEvent::Exited { state } => Some(*state),
            _ => None,
        })
        .collect()
}

fn entry_action_states(j: &[TraceEvent]) -> Vec<u16> {
    j.iter()
        .filter_map(|e| match e {
            TraceEvent::ActionInvoked {
                state,
                kind: ActionKind::Entry,
                ..
            } => Some(*state),
            _ => None,
        })
        .collect()
}

#[tokio::test(flavor = "current_thread")]
async fn det_initial_lands_on_composite_no_descent() {
    // Standby has children but no `default` — initial entry stops at Standby.
    // LowPower must NOT enter.
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Microwave::new(Ctx);
            // Drive initial entry without consuming an event.
            let _ = m.step(hsmc::Duration::ZERO).await;

            assert_eq!(
                m.current_state(),
                MicrowaveState::Standby,
                "innermost active state is the composite Standby"
            );

            let j = m.take_journal();
            let entered = entered_states(&j);
            assert_eq!(
                entered,
                vec![SR, S_STANDBY],
                "initial entry stops at Standby; no default-descent into LowPower"
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_transition_into_substate_works() {
    // Explicit `=> LowPower` from Standby enters LowPower.
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Microwave::new(Ctx);
            let _ = m.dispatch(Ev::EnterLowPower).await;
            assert_eq!(m.current_state(), MicrowaveState::LowPower);
            let j = m.take_journal();
            // Standby entered first (initial), then LowPower entered after the event.
            let entered = entered_states(&j);
            assert_eq!(entered, vec![SR, S_STANDBY, S_LOW]);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_up_from_substate_lands_on_composite() {
    // From LowPower, `=> Standby` is an up-transition: exit LowPower, do not
    // re-enter Standby.
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Microwave::new(Ctx);
            let _ = m.dispatch(Ev::EnterLowPower).await;
            let _ = m.dispatch(Ev::LeaveLowPower).await;
            assert_eq!(m.current_state(), MicrowaveState::Standby);

            let j = m.take_journal();
            // Across the whole run: enter Standby, enter LowPower, exit
            // LowPower. Standby's entry must have run exactly once.
            let entry_states = entry_action_states(&j);
            let standby_entries = entry_states.iter().filter(|&&s| s == S_STANDBY).count();
            assert_eq!(
                standby_entries, 1,
                "Standby's entry runs once; up-transition does not re-enter it"
            );
            let exited = exited_states(&j);
            assert_eq!(
                exited,
                vec![S_LOW],
                "only LowPower is exited; Standby remains on the active path"
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_lateral_to_composite_does_not_descend() {
    // From On, `=> Standby` lands on Standby — NOT on LowPower.
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Microwave::new(Ctx);
            let _ = m.dispatch(Ev::PowerOn).await;
            assert_eq!(m.current_state(), MicrowaveState::On);
            let _ = m.dispatch(Ev::PowerOff).await;
            assert_eq!(
                m.current_state(),
                MicrowaveState::Standby,
                "PowerOff lands on Standby; absence of `default` skips descent"
            );

            let j = m.take_journal();
            // LowPower must never have been entered.
            let entered = entered_states(&j);
            assert!(
                !entered.contains(&S_LOW),
                "no descent into LowPower on entry to Standby"
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_terminate_from_composite_unwinds_path() {
    // Halt while at Standby exits Standby → Root only.
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Microwave::new(Ctx);
            let _ = m.dispatch(Ev::Halt).await;
            let j = m.take_journal();
            let exits = exited_states(&j);
            assert_eq!(
                exits,
                vec![S_STANDBY, SR],
                "terminate from Standby exits Standby then Root"
            );
            assert!(matches!(j.last(), Some(TraceEvent::Terminated)));
        })
        .await;
}
