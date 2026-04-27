#![allow(dead_code)]
//! Deterministic-flow tests for `emit()` from action code.
//!
//! Spec section "emit()":
//!   The event goes on the machine's internal queue. It is NOT processed
//!   immediately. The runtime finishes handling whatever event it's
//!   currently handling — including every entry and exit action of any
//!   transition — and only then dequeues `Ev` and dispatches it normally.
//!   No re-entrant dispatch.
//!
//!   If the queue is full, `emit()` returns `Err(QueueFull)`. Subsequent
//!   actions in the same chain still run; they may attempt further emits,
//!   but those also fail.

#![cfg(all(feature = "tokio", feature = "journal"))]
#![allow(unexpected_cfgs)]

use hsmc::{statechart, ActionKind, TraceEvent};

#[derive(Default)]
pub struct Ctx;

#[derive(Debug, Clone)]
pub enum Ev {
    Trigger,
    Followup,
    Many,
    Halt,
}

statechart! {
    Em {
        context: Ctx;
        events:  Ev;
        default(A);
        terminate(Halt);

        state A {
            entry: a_in;
            exit:  a_out;
            on(Trigger) => emit_followup;
            on(Followup) => B;
            on(Many) => emit_many;
        }
        state B {
            entry: b_in;
            exit:  b_out;
        }
    }
}

impl EmActions for EmActionContext<'_> {
    async fn a_in(&mut self)   {}
    async fn a_out(&mut self)  {}
    async fn b_in(&mut self)   {}
    async fn b_out(&mut self)  {}
    async fn emit_followup(&mut self) {
        let _ = self.emit(Ev::Followup);
    }
    async fn emit_many(&mut self) {
        // Try to overflow the queue; capacity is 8 by default.
        for _ in 0..15 {
            let _ = self.emit(Ev::Followup);
        }
    }
}

const E_TRIGGER: u16 = 0;
const E_FOLLOWUP: u16 = 1;
const E_MANY: u16 = 2;

#[tokio::test(flavor = "current_thread")]
async fn det_emit_queues_event() {
    // emit_followup queues Followup. Journal must have EmitQueued.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Em::new(Ctx);
        let _ = m.dispatch(Ev::Trigger).await;
        let queued = m.journal().iter().filter(|e| matches!(
            e, TraceEvent::EmitQueued { .. }
        )).count();
        assert_eq!(queued, 1, "exactly one EmitQueued, got {:?}", m.journal());
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_emit_followup_event_is_dispatched() {
    // After Trigger fires emit_followup, the queue contains Followup.
    // dispatch() drains, so Followup is dispatched and transitions A → B.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Em::new(Ctx);
        let _ = m.dispatch(Ev::Trigger).await;
        let j = m.take_journal();

        // Both Trigger and Followup must be EventDelivered.
        let delivered_events: Vec<u16> = j.iter().filter_map(|e| match e {
            TraceEvent::EventDelivered { event, .. } => Some(*event),
            _ => None,
        }).collect();
        assert_eq!(delivered_events, vec![E_TRIGGER, E_FOLLOWUP]);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_emit_no_reentrant_dispatch() {
    // Spec: emit during a handler must not re-enter the dispatcher inside the
    // current handler. The emit_followup action does ONE thing: emit. Then it
    // returns. ONLY THEN does the dispatcher pick up Followup.
    //
    // Verify: the EmitQueued event for Followup appears BEFORE the
    // EventDelivered for Followup.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Em::new(Ctx);
        let _ = m.dispatch(Ev::Trigger).await;
        let j = m.take_journal();
        let queued_idx = j.iter().position(|e| matches!(e, TraceEvent::EmitQueued { event: E_FOLLOWUP })).unwrap();
        let delivered_idx = j.iter().position(|e| matches!(
            e, TraceEvent::EventDelivered { event: E_FOLLOWUP, .. }
        )).unwrap();
        assert!(queued_idx < delivered_idx,
            "EmitQueued ({}) must come before EventDelivered ({})", queued_idx, delivered_idx);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_emit_overflow_records_emitfailed() {
    // emit_many tries 15 emits. Default queue cap is 8 (one slot has Many in
    // it currently being processed; 7 more fit). Some succeed, some fail.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Em::new(Ctx);
        let _ = m.dispatch(Ev::Many).await;
        let j = m.take_journal();
        let queued = j.iter().filter(|e| matches!(e, TraceEvent::EmitQueued { .. })).count();
        let failed = j.iter().filter(|e| matches!(e, TraceEvent::EmitFailed { .. })).count();
        assert!(queued > 0, "some emits must succeed");
        assert!(failed > 0, "some emits must fail when queue overflows");
        assert_eq!(queued + failed, 15, "total emit attempts must be 15");
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_emit_does_not_reorder_external_events() {
    // External events still process in arrival order. Emitted events are
    // appended to the queue (FIFO).
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Em::new(Ctx);
        let _ = m.dispatch(Ev::Trigger).await;
        let j = m.take_journal();
        let delivered_events: Vec<u16> = j.iter().filter_map(|e| match e {
            TraceEvent::EventDelivered { event, .. } => Some(*event),
            _ => None,
        }).collect();
        // Trigger was delivered first, then the emitted Followup.
        assert_eq!(delivered_events[0], E_TRIGGER);
        assert_eq!(delivered_events[1], E_FOLLOWUP);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_emit_byte_deterministic() {
    let run = || async {
        tokio::task::LocalSet::new().run_until(async {
            let mut m = Em::new(Ctx);
            let _ = m.dispatch(Ev::Trigger).await;
            let _ = m.dispatch(Ev::Halt).await;
            m.take_journal()
        }).await
    };
    let first = run().await;
    for i in 1..10 {
        assert_eq!(first, run().await, "diverged on run {i}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn det_emit_state_after_emit_action_runs_to_completion() {
    // Spec: subsequent actions in the same chain still run after emit failure.
    // Hard to test surgically without an action-after-emit; pin instead that
    // the EmitFailed events are followed by additional ActionInvoked events
    // for the SAME emit_many handler call.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Em::new(Ctx);
        let _ = m.dispatch(Ev::Many).await;
        let j = m.take_journal();
        // The handler emit_many is one call; it emits 15 times. So we should see
        // 15 emit-result events all between two boundaries: after Many's
        // EventDelivered and before any subsequent Followup deliveries.
        let many_delivered_idx = j.iter().position(|e| matches!(
            e, TraceEvent::EventDelivered { event: E_MANY, .. }
        )).unwrap();
        let emit_results: Vec<&TraceEvent> = j[many_delivered_idx..].iter().take_while(|e| matches!(
            e,
            TraceEvent::EmitQueued { .. }
                | TraceEvent::EmitFailed { .. }
                | TraceEvent::ActionInvoked { .. }
                | TraceEvent::EventDelivered { event: E_MANY, .. }
        )).filter(|e| matches!(
            e, TraceEvent::EmitQueued { .. } | TraceEvent::EmitFailed { .. }
        )).collect();
        assert_eq!(emit_results.len(), 15);
    }).await;
}
