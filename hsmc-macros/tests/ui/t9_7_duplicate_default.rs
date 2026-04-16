use hsmc::statechart;

#[derive(Debug, Clone)]
pub enum Ev { _U }

pub struct Ctx;

statechart! {
M {
    context: Ctx;
    events: Ev;
    default(Parent);
    state Parent {
        default(A);
        default(B);
        state A { entry: e; }
        state B { entry: e; }
    }
}
}

impl MActions for MActionContext<'_> {
    fn e(&mut self) {}
}

fn main() {}
