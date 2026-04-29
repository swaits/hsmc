//! Compile-pass: a state's `default(...)` may target an ancestor. Entering
//! the declaring state would immediately exit back up to the ancestor (the
//! up-transition rule applies — ancestor not re-entered). The state is
//! reached only via an explicit transition; without one, control never
//! enters it. (No cycle here: Outer has no default; Inner targets Outer.)
#![allow(unexpected_cfgs)]

use hsmc::statechart;

#[derive(Debug, Clone)]
pub enum Ev { GoIn, Halt }

pub struct Ctx;

statechart! {
M {
    context: Ctx;
    events:  Ev;
    default(Outer);
    terminate(Halt);

    state Outer {
        entry: outer_in;
        on(GoIn) => Inner;
        state Inner {
            entry: inner_in;
            // Default jumps up to Outer. Inner is reachable only via the
            // explicit GoIn transition; once entered, this default fires
            // and lands the chart back at Outer (no re-entry of Outer).
            default(Outer);
        }
    }
}
}

impl MActions for MActionContext<'_> {
    fn outer_in(&mut self) {}
    fn inner_in(&mut self) {}
}

fn main() {
    let _ = M::new(Ctx);
}
