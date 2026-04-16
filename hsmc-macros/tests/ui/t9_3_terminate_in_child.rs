use hsmc::statechart;

#[derive(Debug, Clone)]
pub enum Ev { Halt }

pub struct Ctx;

statechart! {
M {
    context: Ctx;
    events: Ev;
    default(Child);
    state Child {
        terminate(Halt);
        entry: e;
    }
}
}

impl MActions for MActionContext<'_> {
    fn e(&mut self) {}
}

fn main() {}
