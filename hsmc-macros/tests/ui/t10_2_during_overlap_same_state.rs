//! t10.2: two `during:` declarations on the same state borrowing the same
//! field produce a clear macro-level error.
use hsmc::statechart;

#[derive(Debug, Clone)]
pub enum Ev { Halt }

pub struct Ctx {
    pub a: u32,
}

statechart! {
M {
    context: Ctx;
    events: Ev;
    default(S);
    terminate(Halt);

    state S {
        during: one(a);
        during: two(a);
    }
}
}

async fn one(_: &mut u32) -> Ev { Ev::Halt }
async fn two(_: &mut u32) -> Ev { Ev::Halt }

impl MActions for MActionContext<'_> {}

fn main() {}
