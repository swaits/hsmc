use hsmc::statechart;

#[derive(Debug, Clone)]
pub enum Ev { Halt, Abort }

pub struct Ctx;

statechart! {
M {
    context: Ctx;
    events: Ev;
    default(A);
    terminate(Halt);
    terminate(Abort);
    state A { entry: e; }
}
}

impl MActions for MActionContext<'_> {
    fn e(&mut self) {}
}

fn main() {}
