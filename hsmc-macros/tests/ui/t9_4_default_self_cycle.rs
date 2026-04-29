// `default(self)` is a 1-cycle: entering A would fire its default
// transition to A, which would fire its default again, ad infinitum.
// Compile-time error.
use hsmc::statechart;

#[derive(Debug, Clone)]
pub enum Ev { _U }

pub struct Ctx;

statechart! {
M {
    context: Ctx;
    events: Ev;
    default(A);
    state A {
        default(A);
        entry: e;
    }
}
}

impl MActions for MActionContext<'_> {
    fn e(&mut self) {}
}

fn main() {}
