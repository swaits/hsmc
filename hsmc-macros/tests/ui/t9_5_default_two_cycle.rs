// A → B → A is a default cycle: entering either state would loop
// forever before reaching user code. Compile-time error.
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
        default(B);
        entry: e;
    }
    state B {
        default(A);
        entry: e;
    }
}
}

impl MActions for MActionContext<'_> {
    fn e(&mut self) {}
}

fn main() {}
