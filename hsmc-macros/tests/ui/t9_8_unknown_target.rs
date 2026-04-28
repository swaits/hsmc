#![allow(unexpected_cfgs)]

use hsmc::statechart;

#[derive(Debug, Clone)]
pub enum Ev { Go }

pub struct Ctx;

statechart! {
M {
    context: Ctx;
    events: Ev;
    default(A);
    state A {
        on(Go) => NonExistent;
    }
}
}

impl MActions for MActionContext<'_> {}

fn main() {}
