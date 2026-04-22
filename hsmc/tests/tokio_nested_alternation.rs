//! Regression: a nested `default`-chain toggle (Splash.{LearnMore, Info}) must
//! keep alternating after the parent subtree has been exited to a sibling
//! (Dashboard) and re-entered via the parent's default.
//!
//! The firmware saw: on fresh boot, alternation between the two substates
//! worked perfectly. After one round-trip out to Dashboard and back,
//! alternation stalled. This test reproduces the scenario in async `run()`
//! so we can see it fail without hardware.

#![cfg(feature = "tokio")]

use hsmc::{statechart, Duration};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct Ctx {
    pub log: Arc<Mutex<Vec<&'static str>>>,
}

#[derive(Debug, Clone)]
pub enum Ev {
    Go,
    Back,
    Halt,
}

statechart! {
    M {
        context: Ctx;
        events: Ev;
        default(On);
        terminate(Halt);

        state On {
            on(Go) => Listening;
            on(Back) => Splash;
            default(Splash);

            state Splash {
                default(LearnMore);
                state LearnMore {
                    entry: enter_lm;
                    on(after Duration::from_secs(5)) => Info;
                }
                state Info {
                    entry: enter_info;
                    on(after Duration::from_secs(5)) => LearnMore;
                }
            }

            state Dashboard {
                default(Listening);
                state Listening {
                    entry: enter_listening;
                    // Repeating tick that matches the firmware's
                    // `on(every 1s) => advance_sparkline, paint_listening`.
                    on(every Duration::from_millis(1000)) => tick_listen;
                }
            }
        }
    }
}

impl MActions for MActionContext<'_> {
    async fn enter_lm(&mut self) {
        self.log.lock().unwrap().push("LM");
    }
    async fn enter_info(&mut self) {
        self.log.lock().unwrap().push("INFO");
    }
    async fn enter_listening(&mut self) {
        self.log.lock().unwrap().push("LISTEN");
    }
    async fn tick_listen(&mut self) {
        // Intentionally mirror the firmware: the tick handler does a
        // short async await (simulating a display flush). This is the
        // kind of thing that lengthens `step()` and exposes subtle
        // timing bugs in the run loop.
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn nested_alternation_survives_round_trip() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut m = M::new(Ctx { log: log.clone() });
    let sender = m.sender();

    // Driver: exercise the full scenario, then halt.
    tokio::spawn(async move {
        // Let LM alternate with INFO for three cycles.
        tokio::time::sleep(Duration::from_secs(5)).await; // LM→INFO
        tokio::time::sleep(Duration::from_secs(5)).await; // INFO→LM
        tokio::time::sleep(Duration::from_secs(5)).await; // LM→INFO

        // Jump out to Listening. The machine will now tick every 1s
        // with a 20ms-await tick handler — the firmware's actual shape.
        let _ = sender.send(Ev::Go);
        // Let several ticks fire (simulates a short RX session).
        tokio::time::sleep(Duration::from_millis(3500)).await;

        // Back to Splash. Splash's default should land us in LM.
        let _ = sender.send(Ev::Back);
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Regression: alternation must continue after re-entry.
        tokio::time::sleep(Duration::from_secs(5)).await; // LM→INFO
        tokio::time::sleep(Duration::from_secs(5)).await; // INFO→LM

        // Second round-trip to be sure it wasn't a one-off.
        let _ = sender.send(Ev::Go);
        tokio::time::sleep(Duration::from_millis(2500)).await;
        let _ = sender.send(Ev::Back);
        tokio::time::sleep(Duration::from_millis(50)).await;

        tokio::time::sleep(Duration::from_secs(5)).await; // LM→INFO
        tokio::time::sleep(Duration::from_secs(5)).await; // INFO→LM

        let _ = sender.send(Ev::Halt);
    });

    let res = tokio::time::timeout(Duration::from_secs(300), m.run())
        .await
        .expect("run() hung");
    assert!(res.is_ok(), "run() returned error: {:?}", res);

    let log = log.lock().unwrap();
    // Between LISTEN events, LM and INFO must alternate perfectly.
    // Expected overall: LM INFO LM INFO LISTEN LM INFO LM LISTEN LM INFO LM.
    assert_eq!(
        &log[..],
        &[
            "LM", "INFO", "LM", "INFO",   // fresh-boot alternation ×3
            "LISTEN", // Go #1
            "LM", "INFO", "LM",     // Back #1 + two 5s ticks
            "LISTEN", // Go #2
            "LM", "INFO", "LM", // Back #2 + two 5s ticks
        ][..],
        "log did not match expected sequence: {:?}",
        &log[..]
    );
}
