use hsmc::statechart;

#[derive(Debug, Clone)]
pub enum Ev { _U }

pub struct Ctx;

statechart! {
M {
    context: Ctx;
    events: Ev;
    default(Leaf);
    state Leaf {
        default(Ghost);
        entry: e;
    }
}
}

impl MActions for MActionContext<'_> {
    fn e(&mut self) {}
}

fn main() {}
