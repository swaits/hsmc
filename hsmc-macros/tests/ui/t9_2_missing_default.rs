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
        state ChildA { entry: e; }
        state ChildB { entry: e; }
    }
}
}

impl MActions for MActionContext<'_> {
    fn e(&mut self) {}
}

fn main() {}
