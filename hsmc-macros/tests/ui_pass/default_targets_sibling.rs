//! Compile-pass: a state's `default(...)` may target a sibling. Entering
//! the declaring state immediately fires a transition out to the sibling,
//! using the same LCA-aware algorithm as any other transition.
#![allow(unexpected_cfgs)]

use hsmc::statechart;

#[derive(Debug, Clone)]
pub enum Ev { Halt }

pub struct Ctx;

statechart! {
M {
    context: Ctx;
    events:  Ev;
    default(Foyer);
    terminate(Halt);

    state Foyer {
        // Default redirects out of Foyer to its sibling LivingRoom.
        default(LivingRoom);
        entry: foyer_in;
    }
    state LivingRoom {
        entry: living_in;
    }
}
}

impl MActions for MActionContext<'_> {
    fn foyer_in(&mut self) {}
    fn living_in(&mut self) {}
}

fn main() {
    let _ = M::new(Ctx);
}
