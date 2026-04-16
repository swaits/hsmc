//! Every generated machine exposes a `STATE_CHART: &'static str` constant
//! containing an ASCII tree of its hierarchy. Useful for `defmt::info!`,
//! panic messages, and documentation.

#![cfg(not(any(feature = "tokio", feature = "embassy")))]

use hsmc::{statechart, Duration};

#[derive(Default)]
pub struct Ctx;

#[derive(Debug, Clone)]
pub enum Ev {
    Go,
    Back,
    Halt,
}

statechart! {
Diag {
    context: Ctx;
    events: Ev;
    default(Idle);
    terminate(Halt);

    state Idle {
        entry: enter_idle;
        on(Go) => Running;
    }
    state Running {
        default(Fast);
        on(Back) => Idle;
        state Fast {
            on(after Duration::from_millis(10)) => Slow;
        }
        state Slow {
            on(after Duration::from_millis(20)) => Fast;
        }
    }
}
}

impl DiagActions for DiagActionContext<'_> {
    fn enter_idle(&mut self) {}
}

#[test]
fn state_chart_is_non_empty_and_names_states() {
    // The machine carries a const generic queue capacity; use turbofish to
    // reach the associated const externally.
    let chart = Diag::<8>::STATE_CHART;
    // Starts with the machine name.
    assert!(chart.starts_with("Diag\n"), "chart: {:?}", chart);
    // Names every declared state.
    assert!(chart.contains("Idle"));
    assert!(chart.contains("Running"));
    assert!(chart.contains("Fast"));
    assert!(chart.contains("Slow"));
    // Marks the default child on the root and on Running.
    assert!(chart.contains("[default] Idle"));
    assert!(chart.contains("[default] Fast"));
    // Shows entry action and handlers.
    assert!(chart.contains("entry: enter_idle"));
    assert!(chart.contains("on(Go)"));
}
