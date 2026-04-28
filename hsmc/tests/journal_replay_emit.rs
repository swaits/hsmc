//! Replay test for emit() journaling.

#![cfg(all(feature = "tokio", feature = "journal"))]
#![allow(unexpected_cfgs)]

use hsmc::{statechart, TraceEvent};

#[derive(Default)]
pub struct Ctx;

#[derive(Debug, Clone)]
pub enum Ev {
    Trigger,
    Followup,
    Halt,
}

statechart! {
    EmitProbe {
        context: Ctx;
        events:  Ev;

        default(A);
        terminate(Halt);

        state A {
            on(Trigger) => emit_followup;
            on(Followup) => B;
        }
        state B {
            entry: noop;
        }
    }
}

impl EmitProbeActions for EmitProbeActionContext<'_> {
    async fn emit_followup(&mut self) {
        self.emit(Ev::Followup).unwrap();
    }
    async fn noop(&mut self) {}
}

#[tokio::test(flavor = "current_thread")]
async fn emit_queued_appears_in_journal() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = EmitProbe::new(Ctx::default());
            let _ = m.dispatch(Ev::Trigger).await;

            let saw_emit = m
                .journal()
                .iter()
                .any(|e| matches!(e, TraceEvent::EmitQueued { .. }));
            assert!(
                saw_emit,
                "journal must contain EmitQueued, got {:?}",
                m.journal()
            );

            // The follow-up event MUST be delivered (proving the queue actually
            // drained the emit'd event).
            let delivered_followup = m.journal().iter().any(|e| {
                matches!(
                    e,
                    TraceEvent::EventDelivered { event: 1, .. } // 1 = Followup (Trigger=0, Followup=1)
                )
            });
            assert!(
                delivered_followup,
                "expected Followup to be delivered after emit"
            );
        })
        .await;
}
