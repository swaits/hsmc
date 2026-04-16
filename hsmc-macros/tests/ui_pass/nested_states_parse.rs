//! Compile-pass parse test mirroring spec §8 "Complete Example: Nested States".
//! The only deviation from the spec snippet is the outer-brace macro call syntax.
#![allow(unexpected_cfgs)]

use hsmc::{statechart, Duration};

#[derive(Debug, Clone)]
pub enum MyEvent {
    Event1,
    Event2,
    Halt,
}

#[derive(Default)]
pub struct MyContext {
    pub log: Vec<String>,
}

statechart! {
Name {
    context: MyContext;
    events: MyEvent;

    default(FirstState);
    entry: mainentry, mainentry2, mainentry3;
    exit: mainexit;
    terminate(Halt);

    // Root-level transitions and actions
    on(after Duration::from_secs(1)) => FirstState;
    on(after Duration::from_secs_f64(0.99)) => asldkfj;
    on(after Duration::from_secs_f64(0.99)) => asldkfj1;
    on(after Duration::from_secs_f64(0.99)) => asldkfj2;

    state FirstState {
        default(SubState);
        entry: blah_entry;
        exit: blah_exit;

        on(Event2) => SecondState;
        on(Event1) => x_action;

        state SubState {
            default(SubSubState);

            state SubSubState {
                default(SubSubSubState);

                state SubSubSubState {
                    entry: subsubsub_entry;
                }
            }
        }
    }

    state SecondState {
        on(Event1) => FirstState;
    }

    state XSecondState {
        entry: x_entry;
    }

    state SomeEndState {
        entry: end_entry;
        exit: end_exit;
        on(after Duration::from_secs_f64(0.23)) => something;
    }
}
}

impl NameActions for NameActionContext<'_> {
    fn mainentry(&mut self)  { self.log.push("mainentry".into()); }
    fn mainentry2(&mut self) { self.log.push("mainentry2".into()); }
    fn mainentry3(&mut self) { self.log.push("mainentry3".into()); }
    fn mainexit(&mut self)   { self.log.push("mainexit".into()); }
    fn blah_entry(&mut self) { self.log.push("blah_entry".into()); }
    fn blah_exit(&mut self)  { self.log.push("blah_exit".into()); }
    fn x_action(&mut self)   { self.log.push("x_action".into()); }
    fn asldkfj(&mut self)    { self.log.push("asldkfj".into()); }
    fn asldkfj1(&mut self)   { self.log.push("asldkfj1".into()); }
    fn asldkfj2(&mut self)   { self.log.push("asldkfj2".into()); }
    fn x_entry(&mut self)    { self.log.push("x_entry".into()); }
    fn end_entry(&mut self)  { self.log.push("end_entry".into()); }
    fn end_exit(&mut self)   { self.log.push("end_exit".into()); }
    fn something(&mut self)  { self.log.push("something".into()); }
    fn subsubsub_entry(&mut self) { self.log.push("subsubsub_entry".into()); }
}

fn main() {
    let _ = Name::new(MyContext::default());
}
