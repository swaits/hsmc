//! t10.1: a single `during:` listing the same field twice is a compile error.
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
        during: tick(a, a);
    }
}
}

async fn tick(_: &mut u32, _: &mut u32) -> Ev { Ev::Halt }

impl MActions for MActionContext<'_> {}

fn main() {}
