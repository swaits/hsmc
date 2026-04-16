use hsmc::{statechart, Duration};

#[derive(Debug, Clone)]
pub enum Ev { Go }

pub struct Ctx;

statechart! {
M {
    context: Ctx;
    events: Ev;
    default(Red);
    state Red {
        on(Go) => Green;
        on(Go) => Yellow;
    }
    state Green { entry: e; }
    state Yellow { entry: e; }
}
}

impl MActions for MActionContext<'_> {
    fn e(&mut self) {}
}

fn main() {}
