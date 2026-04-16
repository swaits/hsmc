use hsmc::statechart;

#[derive(Debug, Clone)]
pub enum Ev { _U }

pub struct Ctx;

statechart! {
M {
    context: Ctx;
    events: Ev;
    default(A);
    state A { entry: e; }
    state Empty {}
}
}

impl MActions for MActionContext<'_> {
    fn e(&mut self) {}
}

fn main() {}
