//! Prints the generated STATE_CHART string. Useful as a visual check
//! during development. Run with `cargo test --test state_chart_preview --
//! --nocapture`.

#![cfg(not(any(feature = "tokio", feature = "embassy")))]

use hsmc::{statechart, Duration};

#[derive(Default)]
pub struct Ctx;

#[derive(Debug, Clone)]
pub enum Ev {
    Go,
    Halt,
}

statechart! {
Preview {
    context: Ctx;
    events: Ev;
    default(Idle);
    terminate(Halt);

    state Idle {
        entry: on_idle;
        on(Go) => Running;
    }
    state Running {
        entry: on_run;
        exit: leave_run;
        default(Fast);
        on(Halt) => Running;

        state Fast {
            on(after Duration::from_millis(10)) => Slow;
        }
        state Slow {
            on(after Duration::from_millis(20)) => Fast;
        }
    }
}
}

impl PreviewActions for PreviewActionContext<'_> {
    fn on_idle(&mut self) {}
    fn on_run(&mut self) {}
    fn leave_run(&mut self) {}
}

#[test]
fn preview_state_chart() {
    println!("\n{}", Preview::<8>::STATE_CHART);
}
