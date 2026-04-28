//! Replay test for timer arm + cancel paths.

#![cfg(all(feature = "tokio", feature = "journal"))]
#![allow(unexpected_cfgs)]

use hsmc::{statechart, TraceEvent};

#[derive(Default)]
pub struct Ctx {
    pub fired: u32,
}

#[derive(Debug, Clone)]
pub enum Ev {
    Stop,
    Halt,
}

statechart! {
    TimerProbe {
        context: Ctx;
        events:  Ev;

        default(Counting);
        terminate(Halt);

        state Counting {
            entry: noop;
            exit:  noop2;
            on(after hsmc::Duration::from_millis(100)) => Done;
            on(Stop) => Done;
        }
        state Done {
            entry: noop3;
        }
    }
}

impl TimerProbeActions for TimerProbeActionContext<'_> {
    async fn noop(&mut self) {}
    async fn noop2(&mut self) {}
    async fn noop3(&mut self) {}
}

#[tokio::test(flavor = "current_thread")]
async fn timer_arm_then_cancel_journals_both() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = TimerProbe::new(Ctx::default());
            let _ = m.dispatch(Ev::Stop).await;

            let timer_events: Vec<&TraceEvent> = m
                .journal()
                .iter()
                .filter(|e| {
                    matches!(
                        e,
                        TraceEvent::TimerArmed { .. }
                            | TraceEvent::TimerCancelled { .. }
                            | TraceEvent::TimerFired { .. }
                    )
                })
                .collect();

            assert_eq!(
                timer_events.len(),
                2,
                "expected 2 timer events, got {:?}",
                timer_events
            );
            assert!(
                matches!(
                    timer_events[0],
                    TraceEvent::TimerArmed {
                        state: 1,
                        timer: 0,
                        ns: 100_000_000
                    }
                ),
                "got {:?}",
                timer_events[0]
            );
            assert!(
                matches!(
                    timer_events[1],
                    TraceEvent::TimerCancelled { state: 1, timer: 0 }
                ),
                "got {:?}",
                timer_events[1]
            );
        })
        .await;
}
