//! Dumps a journal for visual inspection. Run with:
//!
//!   cargo test -p hsmc --features tokio,journal --test journal_dump -- --nocapture
//!
//! Not an assertion — just shows the actual sequence so we can sanity-check
//! that "EVERYTHING" is journaled.

#![cfg(all(feature = "tokio", feature = "journal"))]
#![allow(unexpected_cfgs)]

use hsmc::statechart;

#[derive(Default)]
pub struct Ctx {
    pub n: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Ev {
    Go,
    Beep,
    Halt,
}

statechart! {
    Dump {
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
            on(Beep) => beep_at_idle;
        }
        state Active {
            entry: active_in;
            exit:  active_out;
            default(Sub);
            state Sub {
                entry: sub_in;
                exit:  sub_out;
                on(Beep) => beep_at_sub;
            }
        }
    }
}

impl DumpActions for DumpActionContext<'_> {
    async fn root_in(&mut self) {
        self.n += 1;
    }
    async fn root_out(&mut self) {}
    async fn idle_in(&mut self) {}
    async fn idle_out(&mut self) {}
    async fn active_in(&mut self) {}
    async fn active_out(&mut self) {}
    async fn sub_in(&mut self) {}
    async fn sub_out(&mut self) {}
    async fn beep_at_idle(&mut self) {}
    async fn beep_at_sub(&mut self) {}
}

#[tokio::test(flavor = "current_thread")]
async fn dump_journal() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Dump::new(Ctx::default());
            let _ = m.dispatch(Ev::Beep).await;
            let _ = m.dispatch(Ev::Go).await;
            let _ = m.dispatch(Ev::Beep).await;
            let _ = m.dispatch(Ev::Halt).await;

            println!(
                "\n=== JOURNAL DUMP (CHART_HASH={:#x}) ===",
                Dump::<8>::CHART_HASH
            );
            for (i, e) in m.journal().iter().enumerate() {
                println!("  {:>3}: {:?}", i, e);
            }
            println!("=== END ({} events) ===\n", m.journal().len());
        })
        .await;
}
