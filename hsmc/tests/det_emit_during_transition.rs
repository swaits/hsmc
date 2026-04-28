#![allow(dead_code)]
//! Tests that an `emit()` from inside an entry/exit action gets queued
//! BEFORE the transition completes, but is dispatched AFTER.

#![cfg(all(feature = "tokio", feature = "journal"))]
#![allow(unexpected_cfgs)]

use hsmc::{statechart, TraceEvent};

#[derive(Default)]
pub struct Ctx;

#[derive(Debug, Clone)]
pub enum Ev {
    Go,
    Cleanup, // emitted from A's exit action
    Wakeup,  // emitted from B's entry action
    Halt,
}

statechart! {
    Et {
        context: Ctx;
        events:  Ev;
        default(A);
        terminate(Halt);

        on(Cleanup) => log_cleanup;
        on(Wakeup) => log_wakeup;

        state A {
            entry: a_in;
            exit:  emit_cleanup;
            on(Go) => B;
        }
        state B {
            entry: emit_wakeup;
            exit:  b_out;
        }
    }
}

impl EtActions for EtActionContext<'_> {
    async fn a_in(&mut self) {}
    async fn b_out(&mut self) {}
    async fn emit_cleanup(&mut self) {
        let _ = self.emit(Ev::Cleanup);
    }
    async fn emit_wakeup(&mut self) {
        let _ = self.emit(Ev::Wakeup);
    }
    async fn log_cleanup(&mut self) {}
    async fn log_wakeup(&mut self) {}
}

#[tokio::test(flavor = "current_thread")]
async fn det_emit_in_exit_action_runs_after_transition() {
    // Spec: emitted events are processed only after the current event has
    // fully finished — including all entry/exit actions.
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Et::new(Ctx);
            let _ = m.dispatch(Ev::Go).await;
            let j = m.take_journal();

            // Find indices of EmitQueued for Cleanup, EmitQueued for Wakeup, and
            // EventDelivered for each.
            // Event ids in interning order: Cleanup=0, Wakeup=1 (root handlers first),
            // Go=2 (A handler), Halt=3 (terminate at end of build).
            let cleanup_queued = j
                .iter()
                .position(|e| matches!(e, TraceEvent::EmitQueued { event: 0 }));
            let wakeup_queued = j
                .iter()
                .position(|e| matches!(e, TraceEvent::EmitQueued { event: 1 }));
            let cleanup_delivered = j
                .iter()
                .position(|e| matches!(e, TraceEvent::EventDelivered { event: 0, .. }));
            let wakeup_delivered = j
                .iter()
                .position(|e| matches!(e, TraceEvent::EventDelivered { event: 1, .. }));

            assert!(cleanup_queued.is_some(), "Cleanup must be queued");
            assert!(wakeup_queued.is_some(), "Wakeup must be queued");
            assert!(cleanup_delivered.is_some(), "Cleanup must be delivered");
            assert!(wakeup_delivered.is_some(), "Wakeup must be delivered");

            // Both emits queue BEFORE either is delivered (no re-entrant dispatch
            // inside the original Go transition).
            assert!(cleanup_queued.unwrap() < cleanup_delivered.unwrap());
            assert!(
                wakeup_queued.unwrap() < cleanup_delivered.unwrap(),
                "Wakeup queued before Cleanup delivered (transition completes first)"
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_emit_in_exit_action_journals_in_order() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Et::new(Ctx);
            let _ = m.dispatch(Ev::Go).await;
            let j = m.take_journal();

            // Both Cleanup and Wakeup were queued during the Go transition. They
            // are delivered AFTER. Order: Cleanup first (emitted from A's exit
            // which runs first), then Wakeup (emitted from B's entry).
            let queued_events: Vec<u16> = j
                .iter()
                .filter_map(|e| match e {
                    TraceEvent::EmitQueued { event } => Some(*event),
                    _ => None,
                })
                .collect();
            assert_eq!(
                queued_events,
                vec![0, 1],
                "Cleanup (id 0) then Wakeup (id 1)"
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_emit_in_transition_byte_deterministic() {
    let run = || async {
        tokio::task::LocalSet::new()
            .run_until(async {
                let mut m = Et::new(Ctx);
                let _ = m.dispatch(Ev::Go).await;
                m.take_journal()
            })
            .await
    };
    let first = run().await;
    for i in 1..10 {
        assert_eq!(first, run().await, "diverged on run {i}");
    }
}
