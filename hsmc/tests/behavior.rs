//! Behavior tests corresponding to §9 of the spec.
//!
//! Gated to non-async features: under `tokio`/`embassy`, action methods are
//! `async fn` and `step()` is `async`, so these tests — which drive the
//! machine synchronously — would fail to compile. Async behavior is covered
//! by `tokio_run.rs` and, on target hardware, the firmware integration tests.
#![cfg(not(any(feature = "tokio", feature = "embassy")))]

use hsmc::{statechart, Duration};

// Shared test context.
#[derive(Default)]
pub struct TestCtx {
    pub log: Vec<String>,
}

impl TestCtx {
    fn logs(&self) -> Vec<&str> {
        self.log.iter().map(|s| s.as_str()).collect()
    }
}

// ---- T1.1: default state entry on first step ----
mod t1_1 {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { _Unused }

    statechart! {
T11 {
        context: TestCtx;
        events: Ev;
        default(Red);
        entry: do_post;
        state Red {
            entry: red_on;
            exit: red_off;
        }
    }
    }

    impl T11Actions for T11ActionContext<'_> {
        fn do_post(&mut self) { self.log.push("do_post".into()); }
        fn red_on(&mut self) { self.log.push("red_on".into()); }
        fn red_off(&mut self) { self.log.push("red_off".into()); }
    }

    #[test]
    fn t1_1_initial_entry() {
        let mut m = T11::new(TestCtx::default());
        m.step(Duration::ZERO);
        assert_eq!(m.context().logs(), ["do_post", "red_on"]);
        assert_eq!(m.current_state(), T11State::Red);
        assert!(!m.is_terminated());
    }
}

// ---- T1.2: nested default descent ----
mod t1_2 {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { _Unused }

    statechart! {
T12 {
        context: TestCtx;
        events: Ev;
        default(A);
        entry: r_entry;
        state A {
            entry: a_entry;
            default(B);
            state B {
                entry: b_entry;
                default(C);
                state C { entry: c_entry; }
            }
        }
    }
    }

    impl T12Actions for T12ActionContext<'_> {
        fn r_entry(&mut self) { self.log.push("r_entry".into()); }
        fn a_entry(&mut self) { self.log.push("a_entry".into()); }
        fn b_entry(&mut self) { self.log.push("b_entry".into()); }
        fn c_entry(&mut self) { self.log.push("c_entry".into()); }
    }

    #[test]
    fn t1_2_nested_default() {
        let mut m = T12::new(TestCtx::default());
        m.step(Duration::ZERO);
        assert_eq!(m.context().logs(), ["r_entry", "a_entry", "b_entry", "c_entry"]);
        assert_eq!(m.current_state(), T12State::C);
    }
}

// ---- T2.1 / T2.6: event-driven sibling transition + self transition ----
mod t2 {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { GoGreen, Reset }

    statechart! {
T2A {
        context: TestCtx;
        events: Ev;
        default(Red);
        state Red {
            entry: red_on;
            exit: red_off;
            on(GoGreen) => Green;
            on(Reset) => Red;
            on(after Duration::from_millis(200)) => Green;
        }
        state Green {
            entry: green_on;
            exit: green_off;
        }
    }
    }

    impl T2AActions for T2AActionContext<'_> {
        fn red_on(&mut self) { self.log.push("red_on".into()); }
        fn red_off(&mut self) { self.log.push("red_off".into()); }
        fn green_on(&mut self) { self.log.push("green_on".into()); }
        fn green_off(&mut self) { self.log.push("green_off".into()); }
    }

    #[test]
    fn t2_1_sibling_transition() {
        let mut m = T2A::new(TestCtx::default());
        m.step(Duration::ZERO); // initial
        m.send(Ev::GoGreen).unwrap();
        m.step(Duration::ZERO);
        assert_eq!(m.current_state(), T2AState::Green);
        let logs = m.context().logs();
        assert!(logs.contains(&"red_off"));
        assert!(logs.contains(&"green_on"));
    }

    #[test]
    fn t2_6_self_transition() {
        let mut m = T2A::new(TestCtx::default());
        m.step(Duration::ZERO);
        m.context_mut().log.clear();
        // Advance 100ms — timer to Green (at 200ms) has 100ms left and must not fire.
        m.step(Duration::from_millis(100));
        assert_eq!(m.current_state(), T2AState::Red);
        // Self-transition resets the 200ms timer.
        m.send(Ev::Reset).unwrap();
        m.step(Duration::ZERO);
        assert_eq!(m.current_state(), T2AState::Red);
        assert_eq!(m.context().logs(), ["red_off", "red_on"]);
        // Only 150ms elapsed since the timer restart — no transition yet.
        m.step(Duration::from_millis(150));
        assert_eq!(m.current_state(), T2AState::Red);
        // 60 more ms pushes past the 200ms deadline.
        m.step(Duration::from_millis(60));
        assert_eq!(m.current_state(), T2AState::Green);
    }
}

// ---- T3.3: actions fire before transition on same trigger ----
mod t3_3 {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { Go }

    statechart! {
T33 {
        context: TestCtx;
        events: Ev;
        default(Red);
        state Red {
            exit: red_off;
            on(Go) => log_go;
            on(Go) => prepare;
            on(Go) => Green;
        }
        state Green { entry: green_on; }
    }
    }

    impl T33Actions for T33ActionContext<'_> {
        fn log_go(&mut self) { self.log.push("log_go".into()); }
        fn prepare(&mut self) { self.log.push("prepare".into()); }
        fn red_off(&mut self) { self.log.push("red_off".into()); }
        fn green_on(&mut self) { self.log.push("green_on".into()); }
    }

    #[test]
    fn t3_3_actions_before_transition() {
        let mut m = T33::new(TestCtx::default());
        m.step(Duration::ZERO);
        m.send(Ev::Go).unwrap();
        m.step(Duration::ZERO);
        assert_eq!(m.context().logs(), ["log_go", "prepare", "red_off", "green_on"]);
        assert_eq!(m.current_state(), T33State::Green);
    }
}

// ---- T2.2 / T8.1: Timer-driven transition ----
mod t2_2 {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { _Unused }

    statechart! {
T22 {
        context: TestCtx;
        events: Ev;
        default(Red);
        state Red { on(after Duration::from_secs(5)) => Green; exit: red_off; }
        state Green { entry: green_on; }
    }
    }

    impl T22Actions for T22ActionContext<'_> {
        fn red_off(&mut self) { self.log.push("red_off".into()); }
        fn green_on(&mut self) { self.log.push("green_on".into()); }
    }

    #[test]
    fn t2_2_timer_transition() {
        let mut m = T22::new(TestCtx::default());
        m.step(Duration::ZERO);
        assert_eq!(m.current_state(), T22State::Red);
        m.step(Duration::from_secs(4));
        assert_eq!(m.current_state(), T22State::Red);
        m.step(Duration::from_secs(1));
        assert_eq!(m.current_state(), T22State::Green);
    }
}

// ---- T4.2: event bubbles to ancestor ----
mod t4_2 {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { Foo }

    statechart! {
T42 {
        context: TestCtx;
        events: Ev;
        default(A);
        state A {
            default(B);
            exit: a_exit;
            on(Foo) => D;
            state B {
                default(C);
                exit: b_exit;
                state C { exit: c_exit; }
            }
        }
        state D { entry: d_entry; }
    }
    }

    impl T42Actions for T42ActionContext<'_> {
        fn a_exit(&mut self) { self.log.push("a_exit".into()); }
        fn b_exit(&mut self) { self.log.push("b_exit".into()); }
        fn c_exit(&mut self) { self.log.push("c_exit".into()); }
        fn d_entry(&mut self) { self.log.push("d_entry".into()); }
    }

    #[test]
    fn t4_2_bubble() {
        let mut m = T42::new(TestCtx::default());
        m.step(Duration::ZERO);
        assert_eq!(m.current_state(), T42State::C);
        m.send(Ev::Foo).unwrap();
        m.step(Duration::ZERO);
        assert_eq!(m.current_state(), T42State::D);
        assert_eq!(m.context().logs(), ["c_exit", "b_exit", "a_exit", "d_entry"]);
    }
}

// ---- T6.1: terminate ----
mod t6_1 {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { Halt }

    statechart! {
T61 {
        context: TestCtx;
        events: Ev;
        default(A);
        terminate(Halt);
        exit: root_exit;
        state A {
            default(B);
            exit: a_exit;
            state B {
                default(C);
                exit: b_exit;
                state C { exit: c_exit; }
            }
        }
    }
    }

    impl T61Actions for T61ActionContext<'_> {
        fn root_exit(&mut self) { self.log.push("root_exit".into()); }
        fn a_exit(&mut self) { self.log.push("a_exit".into()); }
        fn b_exit(&mut self) { self.log.push("b_exit".into()); }
        fn c_exit(&mut self) { self.log.push("c_exit".into()); }
    }

    #[test]
    fn t6_1_terminate() {
        let mut m = T61::new(TestCtx::default());
        m.step(Duration::ZERO);
        m.send(Ev::Halt).unwrap();
        m.step(Duration::ZERO);
        assert!(m.is_terminated());
        assert_eq!(m.context().logs(), ["c_exit", "b_exit", "a_exit", "root_exit"]);
        assert!(m.step(Duration::ZERO).is_none());
    }
}

// ---- T7.1: emit ----
mod t7_1 {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { Foo, Bar }

    statechart! {
T71 {
        context: TestCtx;
        events: Ev;
        default(A);
        state A {
            on(Foo) => emit_bar;
            on(Bar) => B;
        }
        state B { entry: b_entry; }
    }
    }

    impl T71Actions for T71ActionContext<'_> {
        fn emit_bar(&mut self) {
            self.log.push("emit_bar".into());
            self.emit(Ev::Bar).unwrap();
        }
        fn b_entry(&mut self) { self.log.push("b_entry".into()); }
    }

    #[test]
    fn t7_1_emit() {
        let mut m = T71::new(TestCtx::default());
        m.step(Duration::ZERO);
        m.send(Ev::Foo).unwrap();
        m.step(Duration::ZERO); // processes Foo, emits Bar
        assert_eq!(m.current_state(), T71State::A);
        m.step(Duration::ZERO); // processes Bar
        assert_eq!(m.current_state(), T71State::B);
        assert_eq!(m.context().logs(), ["emit_bar", "b_entry"]);
    }
}

// ---- T1.3: machine not started until first step ----
mod t1_3 {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { _U }

    statechart! {
T13 {
        context: TestCtx;
        events: Ev;
        default(Red);
        entry: r_entry;
        state Red { entry: red_on; }
    }
    }

    impl T13Actions for T13ActionContext<'_> {
        fn r_entry(&mut self) { self.log.push("r_entry".into()); }
        fn red_on(&mut self) { self.log.push("red_on".into()); }
    }

    #[test]
    fn t1_3_not_started() {
        let m = T13::new(TestCtx::default());
        assert!(!m.is_terminated());
        assert!(m.context().log.is_empty());
    }
}

// ---- T2.3 / T8.2: timer cancelled on state exit ----
mod t2_3 {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { EarlyExit }

    statechart! {
T23 {
        context: TestCtx;
        events: Ev;
        default(Red);
        state Red {
            on(after Duration::from_secs(5)) => Green;
            on(EarlyExit) => Yellow;
        }
        state Green { entry: green_on; }
        state Yellow { entry: yellow_on; }
    }
    }

    impl T23Actions for T23ActionContext<'_> {
        fn green_on(&mut self) { self.log.push("green_on".into()); }
        fn yellow_on(&mut self) { self.log.push("yellow_on".into()); }
    }

    #[test]
    fn t2_3_timer_cancel_on_exit() {
        let mut m = T23::new(TestCtx::default());
        m.step(Duration::ZERO);
        m.step(Duration::from_secs(3));
        m.send(Ev::EarlyExit).unwrap();
        m.step(Duration::ZERO);
        assert_eq!(m.current_state(), T23State::Yellow);
        m.step(Duration::from_secs(5));
        assert_eq!(m.current_state(), T23State::Yellow);
    }
}

// ---- T2.4: cross-hierarchy transition ----
mod t2_4 {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { Jump }

    statechart! {
T24 {
        context: TestCtx;
        events: Ev;
        default(A);
        state A {
            default(B);
            exit: a_exit;
            state B {
                default(C);
                exit: b_exit;
                state C {
                    exit: c_exit;
                    on(Jump) => D;
                }
            }
        }
        state D { entry: d_entry; }
    }
    }

    impl T24Actions for T24ActionContext<'_> {
        fn a_exit(&mut self) { self.log.push("a_exit".into()); }
        fn b_exit(&mut self) { self.log.push("b_exit".into()); }
        fn c_exit(&mut self) { self.log.push("c_exit".into()); }
        fn d_entry(&mut self) { self.log.push("d_entry".into()); }
    }

    #[test]
    fn t2_4_cross_hierarchy() {
        let mut m = T24::new(TestCtx::default());
        m.step(Duration::ZERO);
        m.send(Ev::Jump).unwrap();
        m.step(Duration::ZERO);
        assert_eq!(m.current_state(), T24State::D);
        assert_eq!(m.context().logs(), ["c_exit", "b_exit", "a_exit", "d_entry"]);
    }
}

// ---- T2.5: transition to state with children descends to default ----
mod t2_5 {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { Go }

    statechart! {
T25 {
        context: TestCtx;
        events: Ev;
        default(A);
        state A { on(Go) => B; }
        state B {
            default(C);
            entry: b_entry;
            state C {
                default(D);
                entry: c_entry;
                state D { entry: d_entry; }
            }
        }
    }
    }

    impl T25Actions for T25ActionContext<'_> {
        fn b_entry(&mut self) { self.log.push("b_entry".into()); }
        fn c_entry(&mut self) { self.log.push("c_entry".into()); }
        fn d_entry(&mut self) { self.log.push("d_entry".into()); }
    }

    #[test]
    fn t2_5_descend_to_default() {
        let mut m = T25::new(TestCtx::default());
        m.step(Duration::ZERO);
        m.send(Ev::Go).unwrap();
        m.step(Duration::ZERO);
        assert_eq!(m.current_state(), T25State::D);
        assert_eq!(m.context().logs(), ["b_entry", "c_entry", "d_entry"]);
    }
}

// ---- T2.7: transition from child to sibling of parent ----
mod t2_7 {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { Go }

    statechart! {
T27 {
        context: TestCtx;
        events: Ev;
        default(A);
        state A {
            default(B);
            exit: a_exit;
            state B { exit: b_exit; on(Go) => C; }
        }
        state C { entry: c_entry; }
    }
    }

    impl T27Actions for T27ActionContext<'_> {
        fn a_exit(&mut self) { self.log.push("a_exit".into()); }
        fn b_exit(&mut self) { self.log.push("b_exit".into()); }
        fn c_entry(&mut self) { self.log.push("c_entry".into()); }
    }

    #[test]
    fn t2_7_child_to_uncle() {
        let mut m = T27::new(TestCtx::default());
        m.step(Duration::ZERO);
        m.send(Ev::Go).unwrap();
        m.step(Duration::ZERO);
        assert_eq!(m.current_state(), T27State::C);
        assert_eq!(m.context().logs(), ["b_exit", "a_exit", "c_entry"]);
    }
}

// ---- T2.8: transition to an ancestor ----
mod t2_8 {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { Up }

    statechart! {
T28 {
        context: TestCtx;
        events: Ev;
        default(A);
        state A {
            default(B);
            entry: a_entry;
            exit: a_exit;
            state B {
                entry: b_entry;
                exit: b_exit;
                on(Up) => A;
            }
        }
    }
    }

    impl T28Actions for T28ActionContext<'_> {
        fn a_entry(&mut self) { self.log.push("a_entry".into()); }
        fn a_exit(&mut self) { self.log.push("a_exit".into()); }
        fn b_entry(&mut self) { self.log.push("b_entry".into()); }
        fn b_exit(&mut self) { self.log.push("b_exit".into()); }
    }

    #[test]
    fn t2_8_transition_to_ancestor() {
        let mut m = T28::new(TestCtx::default());
        m.step(Duration::ZERO);
        assert_eq!(m.context().logs(), ["a_entry", "b_entry"]);
        m.context_mut().log.clear();
        m.send(Ev::Up).unwrap();
        m.step(Duration::ZERO);
        assert_eq!(m.current_state(), T28State::B);
        assert_eq!(m.context().logs(), ["b_exit", "a_exit", "a_entry", "b_entry"]);
    }
}

// ---- T3.1 / T3.2: actions in declaration order, no state change ----
mod t3_12 {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { Beep }

    statechart! {
T312 {
        context: TestCtx;
        events: Ev;
        default(Red);
        state Red {
            on(Beep) => honk1;
            on(Beep) => honk2;
            on(Beep) => honk3;
        }
    }
    }

    impl T312Actions for T312ActionContext<'_> {
        fn honk1(&mut self) { self.log.push("honk1".into()); }
        fn honk2(&mut self) { self.log.push("honk2".into()); }
        fn honk3(&mut self) { self.log.push("honk3".into()); }
    }

    #[test]
    fn t3_1_t3_2_actions() {
        let mut m = T312::new(TestCtx::default());
        m.step(Duration::ZERO);
        m.send(Ev::Beep).unwrap();
        m.step(Duration::ZERO);
        assert_eq!(m.context().logs(), ["honk1", "honk2", "honk3"]);
        assert_eq!(m.current_state(), T312State::Red);
    }
}

// ---- T3.4: timer-driven action ----
mod t3_4 {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { _U }

    statechart! {
T34 {
        context: TestCtx;
        events: Ev;
        default(Red);
        state Red {
            on(after Duration::from_millis(500)) => heartbeat;
        }
    }
    }

    impl T34Actions for T34ActionContext<'_> {
        fn heartbeat(&mut self) { self.log.push("beat".into()); }
    }

    #[test]
    fn t3_4_timer_action() {
        let mut m = T34::new(TestCtx::default());
        m.step(Duration::ZERO);
        m.step(Duration::from_millis(500));
        assert_eq!(m.context().logs(), ["beat"]);
        assert_eq!(m.current_state(), T34State::Red);
        // Should not restart.
        m.step(Duration::from_millis(500));
        m.step(Duration::from_millis(500));
        assert_eq!(m.context().logs(), ["beat"]);
    }
}

// ---- T4.1 / T4.3 / T4.4 / T4.5 ----
mod t4_misc {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { Foo, Unknown }

    statechart! {
T4M {
        context: TestCtx;
        events: Ev;
        default(A);
        on(Foo) => root_handler;
        state A {
            default(B);
            state B {
                default(C);
                state C { entry: c_entry; }
            }
        }
    }
    }

    impl T4MActions for T4MActionContext<'_> {
        fn root_handler(&mut self) { self.log.push("root".into()); }
        fn c_entry(&mut self) { /* nop */ }
    }

    #[test]
    fn t4_3_bubble_to_root() {
        let mut m = T4M::new(TestCtx::default());
        m.step(Duration::ZERO);
        m.send(Ev::Foo).unwrap();
        m.step(Duration::ZERO);
        assert_eq!(m.context().logs(), ["root"]);
    }

    #[test]
    fn t4_5_unknown_event_discarded() {
        let mut m = T4M::new(TestCtx::default());
        m.step(Duration::ZERO);
        m.send(Ev::Unknown).unwrap();
        m.step(Duration::ZERO);
        assert!(m.context().log.is_empty());
        assert_eq!(m.current_state(), T4MState::C);
    }
}

mod t4_14 {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { Foo }

    statechart! {
T414 {
        context: TestCtx;
        events: Ev;
        default(A);
        state A {
            default(C);
            on(Foo) => E;
            state C { on(Foo) => D; }
        }
        state D { entry: d_entry; }
        state E { entry: e_entry; }
    }
    }

    impl T414Actions for T414ActionContext<'_> {
        fn d_entry(&mut self) { self.log.push("d_entry".into()); }
        fn e_entry(&mut self) { self.log.push("e_entry".into()); }
    }

    #[test]
    fn t4_1_and_t4_4_leaf_shadows() {
        let mut m = T414::new(TestCtx::default());
        m.step(Duration::ZERO);
        assert_eq!(m.current_state(), T414State::C);
        m.send(Ev::Foo).unwrap();
        m.step(Duration::ZERO);
        assert_eq!(m.current_state(), T414State::D);
        assert_eq!(m.context().logs(), ["d_entry"]);
    }
}

// ---- T4.6: timer triggers do NOT bubble ----
mod t4_6 {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { _U }

    statechart! {
T46 {
        context: TestCtx;
        events: Ev;
        default(A);
        state A {
            default(B);
            on(after Duration::from_secs(10)) => D;
            state B {
                default(C);
                state C { entry: c_entry; }
            }
        }
        state D { entry: d_entry; }
    }
    }

    impl T46Actions for T46ActionContext<'_> {
        fn d_entry(&mut self) { self.log.push("d_entry".into()); }
        fn c_entry(&mut self) { /* nop */ }
    }

    #[test]
    fn t4_6_timer_fires_for_owner() {
        let mut m = T46::new(TestCtx::default());
        m.step(Duration::ZERO);
        assert_eq!(m.current_state(), T46State::C);
        m.step(Duration::from_secs(10));
        assert_eq!(m.current_state(), T46State::D);
    }
}

// ---- T5.1 / T5.2 / T5.3 / T5.4 ordering ----
mod t5 {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { Go }

    statechart! {
T5X {
        context: TestCtx;
        events: Ev;
        default(A);
        state A {
            default(B);
            entry: alpha, beta;
            entry: gamma;
            exit: cleanup1;
            exit: cleanup2;
            state B {
                entry: b_entry;
                exit: b_exit;
                on(Go) => Z;
            }
        }
        state Z { entry: z_entry; }
    }
    }

    impl T5XActions for T5XActionContext<'_> {
        fn alpha(&mut self) { self.log.push("alpha".into()); }
        fn beta(&mut self) { self.log.push("beta".into()); }
        fn gamma(&mut self) { self.log.push("gamma".into()); }
        fn cleanup1(&mut self) { self.log.push("cleanup1".into()); }
        fn cleanup2(&mut self) { self.log.push("cleanup2".into()); }
        fn b_entry(&mut self) { self.log.push("b_entry".into()); }
        fn b_exit(&mut self) { self.log.push("b_exit".into()); }
        fn z_entry(&mut self) { self.log.push("z_entry".into()); }
    }

    #[test]
    fn t5_1_t5_3_entry_order() {
        let mut m = T5X::new(TestCtx::default());
        m.step(Duration::ZERO);
        assert_eq!(m.context().logs(), ["alpha", "beta", "gamma", "b_entry"]);
    }

    #[test]
    fn t5_2_t5_4_exit_order() {
        let mut m = T5X::new(TestCtx::default());
        m.step(Duration::ZERO);
        m.context_mut().log.clear();
        m.send(Ev::Go).unwrap();
        m.step(Duration::ZERO);
        assert_eq!(m.context().logs(), ["b_exit", "cleanup1", "cleanup2", "z_entry"]);
    }
}

// ---- T5.5: LCA not exited/re-entered ----
mod t5_5 {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { Go }

    statechart! {
T55 {
        context: TestCtx;
        events: Ev;
        default(Parent);
        state Parent {
            default(A);
            entry: parent_entry;
            exit: parent_exit;
            on(after Duration::from_secs(10)) => Somewhere;
            state A { entry: a_entry; exit: a_exit; on(Go) => B; }
            state B { entry: b_entry; }
        }
        state Somewhere { entry: somewhere_entry; }
    }
    }

    impl T55Actions for T55ActionContext<'_> {
        fn parent_entry(&mut self) { self.log.push("parent_entry".into()); }
        fn parent_exit(&mut self) { self.log.push("parent_exit".into()); }
        fn a_entry(&mut self) { self.log.push("a_entry".into()); }
        fn a_exit(&mut self) { self.log.push("a_exit".into()); }
        fn b_entry(&mut self) { self.log.push("b_entry".into()); }
        fn somewhere_entry(&mut self) { self.log.push("somewhere_entry".into()); }
    }

    #[test]
    fn t5_5_lca_stays_active() {
        let mut m = T55::new(TestCtx::default());
        // After initial entry, Parent's 10s timer is running. Burn 3s first.
        let next = m.step(Duration::ZERO);
        assert!(next.is_some());
        m.step(Duration::from_secs(3));
        m.context_mut().log.clear();
        m.send(Ev::Go).unwrap();
        let next_after = m.step(Duration::ZERO);
        assert_eq!(m.context().logs(), ["a_exit", "b_entry"]);
        // Parent's timer should still be live with ~7s remaining — not reset to 10s.
        let remaining = next_after.expect("parent timer still armed");
        assert!(
            remaining <= Duration::from_secs(7) + Duration::from_millis(50),
            "parent timer should not have been restarted; remaining={:?}",
            remaining
        );
        assert!(
            remaining >= Duration::from_secs(6),
            "parent timer should have ~7s left; remaining={:?}",
            remaining
        );
    }
}

// ---- T6.2: terminated machine rejects events ----
mod t6_2 {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { Halt, Other }

    statechart! {
T62 {
        context: TestCtx;
        events: Ev;
        default(A);
        terminate(Halt);
        state A { entry: a_entry; }
    }
    }

    impl T62Actions for T62ActionContext<'_> {
        fn a_entry(&mut self) { self.log.push("a".into()); }
    }

    #[test]
    fn t6_2_rejects_after_term() {
        let mut m = T62::new(TestCtx::default());
        m.step(Duration::ZERO);
        m.send(Ev::Halt).unwrap();
        m.step(Duration::ZERO);
        assert!(m.is_terminated());
        let r = m.send(Ev::Other);
        assert!(r.is_err());
    }
}

// ---- T6.4: step() after termination ----
mod t6_4 {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { Halt }

    statechart! {
T64 {
        context: TestCtx;
        events: Ev;
        default(A);
        terminate(Halt);
        state A { entry: a_entry; }
    }
    }

    impl T64Actions for T64ActionContext<'_> {
        fn a_entry(&mut self) { self.log.push("a".into()); }
    }

    #[test]
    fn t6_4_step_after_term() {
        let mut m = T64::new(TestCtx::default());
        m.step(Duration::ZERO);
        m.send(Ev::Halt).unwrap();
        m.step(Duration::ZERO);
        let before = m.context().logs().len();
        let r = m.step(Duration::from_secs(5));
        assert!(r.is_none());
        assert_eq!(m.context().logs().len(), before);
    }
}

// ---- T7.2: emit during entry/exit ----
mod t7_2 {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { Go, Cleanup }

    statechart! {
T72 {
        context: TestCtx;
        events: Ev;
        default(A);
        on(Cleanup) => do_cleanup;
        state A {
            exit: emit_cleanup;
            on(Go) => B;
        }
        state B { entry: b_entry; }
    }
    }

    impl T72Actions for T72ActionContext<'_> {
        fn emit_cleanup(&mut self) {
            self.log.push("a_exit".into());
            self.emit(Ev::Cleanup).unwrap();
        }
        fn b_entry(&mut self) { self.log.push("b_entry".into()); }
        fn do_cleanup(&mut self) { self.log.push("do_cleanup".into()); }
    }

    #[test]
    fn t7_2_emit_during_transition() {
        let mut m = T72::new(TestCtx::default());
        m.step(Duration::ZERO);
        m.send(Ev::Go).unwrap();
        m.step(Duration::ZERO); // process Go; A_exit emits Cleanup; B_entry
        assert_eq!(m.current_state(), T72State::B);
        m.step(Duration::ZERO); // process Cleanup
        assert_eq!(m.context().logs(), ["a_exit", "b_entry", "do_cleanup"]);
    }
}

// ---- T7.3: queue overflow from emit ----
mod t7_3 {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { Start, Tick }

    statechart! {
T73 {
        context: TestCtx;
        events: Ev;
        default(A);
        state A {
            on(Start) => flood;
        }
    }
    }

    impl T73Actions for T73ActionContext<'_> {
        fn flood(&mut self) {
            // Queue capacity is 8 (via with_queue_capacity). Emit 9 times.
            let mut errors = 0;
            for _ in 0..9 {
                if self.emit(Ev::Tick).is_err() { errors += 1; }
            }
            self.log.push(format!("err={}", errors));
        }
    }

    #[test]
    fn t7_3_queue_overflow() {
        let mut m = T73::with_queue_capacity::<8>(TestCtx::default());
        m.step(Duration::ZERO);
        m.send(Ev::Start).unwrap();
        m.step(Duration::ZERO);
        assert!(m.context().log.iter().any(|s| s.starts_with("err=")));
        let got = m.context().log.last().unwrap().clone();
        assert_ne!(got, "err=0");
    }
}

// ---- T8.3: parent timer survives child transitions ----
mod t8_3 {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { _U }

    statechart! {
T83 {
        context: TestCtx;
        events: Ev;
        default(Parent);
        state Parent {
            default(ChildA);
            exit: parent_exit;
            on(after Duration::from_secs(10)) => Done;
            state ChildA {
                exit: childa_exit;
                on(after Duration::from_secs(3)) => ChildB;
            }
            state ChildB { exit: childb_exit; entry: childb_entry; }
        }
        state Done { entry: done_entry; }
    }
    }

    impl T83Actions for T83ActionContext<'_> {
        fn parent_exit(&mut self) { self.log.push("parent_exit".into()); }
        fn childa_exit(&mut self) { self.log.push("childa_exit".into()); }
        fn childb_exit(&mut self) { self.log.push("childb_exit".into()); }
        fn childb_entry(&mut self) { self.log.push("childb_entry".into()); }
        fn done_entry(&mut self) { self.log.push("done_entry".into()); }
    }

    #[test]
    fn t8_3_parent_timer_survives() {
        let mut m = T83::new(TestCtx::default());
        m.step(Duration::ZERO);
        m.step(Duration::from_secs(3));
        assert_eq!(m.current_state(), T83State::ChildB);
        m.step(Duration::from_secs(7));
        assert_eq!(m.current_state(), T83State::Done);
        let logs = m.context().logs();
        // Expect childb_exit, parent_exit, done_entry in that order at the end.
        let tail: Vec<&str> = logs.iter().rev().take(3).rev().cloned().collect();
        assert_eq!(tail, ["childb_exit", "parent_exit", "done_entry"]);
    }
}

// ---- T8.4: timer restarts on re-entry ----
mod t8_4 {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { _U }

    statechart! {
T84 {
        context: TestCtx;
        events: Ev;
        default(Red);
        state Red { on(after Duration::from_secs(5)) => Green; }
        state Green { on(after Duration::from_secs(5)) => Red; }
    }
    }

    impl T84Actions for T84ActionContext<'_> {}

    #[test]
    fn t8_4_timer_restart() {
        let mut m = T84::new(TestCtx::default());
        m.step(Duration::ZERO);
        m.step(Duration::from_secs(5));
        assert_eq!(m.current_state(), T84State::Green);
        m.step(Duration::from_secs(5));
        assert_eq!(m.current_state(), T84State::Red);
        m.step(Duration::from_secs(4));
        assert_eq!(m.current_state(), T84State::Red);
        m.step(Duration::from_secs(1));
        assert_eq!(m.current_state(), T84State::Green);
    }
}

// ---- T8.4b: alternation via shared trigger survives a round-trip to a
//       sibling subtree and back (regression for firmware splash bug). ----
mod t8_4b {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev {
        Go,
        Back,
    }

    statechart! {
    T84b {
        context: TestCtx;
        events: Ev;
        default(On);

        state On {
            on(Go) => Listening;
            on(Back) => Splash;
            default(Splash);

            state Splash {
                default(LearnMore);
                state LearnMore {
                    entry: enter_lm;
                    on(after Duration::from_secs(5)) => Info;
                }
                state Info {
                    entry: enter_info;
                    on(after Duration::from_secs(5)) => LearnMore;
                }
            }

            state Dashboard {
                default(Listening);
                state Listening {
                    entry: enter_listening;
                }
            }
        }
    }
    }

    impl T84bActions for T84bActionContext<'_> {
        fn enter_lm(&mut self) {
            self.log.push("LM".into());
        }
        fn enter_info(&mut self) {
            self.log.push("INFO".into());
        }
        fn enter_listening(&mut self) {
            self.log.push("LISTEN".into());
        }
    }

    #[test]
    fn round_trip_restarts_alternation() {
        let mut m = T84b::new(TestCtx::default());
        m.step(Duration::ZERO);
        assert_eq!(m.current_state(), T84bState::LearnMore);

        // Fresh alternation works.
        m.step(Duration::from_secs(5));
        assert_eq!(m.current_state(), T84bState::Info);
        m.step(Duration::from_secs(5));
        assert_eq!(m.current_state(), T84bState::LearnMore);
        m.step(Duration::from_secs(5));
        assert_eq!(m.current_state(), T84bState::Info);

        // Jump out to a sibling subtree then back.
        m.send(Ev::Go).unwrap();
        m.step(Duration::ZERO);
        assert_eq!(m.current_state(), T84bState::Listening);

        m.send(Ev::Back).unwrap();
        m.step(Duration::ZERO);
        assert_eq!(
            m.current_state(),
            T84bState::LearnMore,
            "after Back we should land in Splash's default (LearnMore)"
        );

        // Now alternation must work again — this is the firmware bug.
        m.step(Duration::from_secs(5));
        assert_eq!(
            m.current_state(),
            T84bState::Info,
            "LearnMore's 5s timer should fire after re-entry"
        );
        m.step(Duration::from_secs(5));
        assert_eq!(
            m.current_state(),
            T84bState::LearnMore,
            "Info's 5s timer should fire after its first re-entry"
        );
    }
}

// ---- T8.5: multiple timers in different hierarchy levels ----
mod t8_5 {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { _U }

    statechart! {
T85 {
        context: TestCtx;
        events: Ev;
        default(A);
        state A {
            default(B);
            on(after Duration::from_secs(2)) => ping;
            state B { on(after Duration::from_secs(5)) => C; }
            state C { entry: c_entry; }
        }
    }
    }

    impl T85Actions for T85ActionContext<'_> {
        fn ping(&mut self) { self.log.push("ping".into()); }
        fn c_entry(&mut self) { self.log.push("c_entry".into()); }
    }

    #[test]
    fn t8_5_multi_level_timers() {
        let mut m = T85::new(TestCtx::default());
        m.step(Duration::ZERO);
        m.step(Duration::from_secs(2));
        assert_eq!(m.current_state(), T85State::B);
        assert_eq!(m.context().logs(), ["ping"]);
        m.step(Duration::from_secs(3));
        assert_eq!(m.current_state(), T85State::C);
        // Verify A's timer does not re-fire
        m.step(Duration::from_secs(10));
        let count = m.context().log.iter().filter(|s| s.as_str() == "ping").count();
        assert_eq!(count, 1);
    }
}

// ---- T10.2: timer-only machine ----
mod t10_2 {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum NoEvents {}

    statechart! {
TimerOnly {
        context: TestCtx;
        events: NoEvents;
        default(A);
        state A { on(after Duration::from_secs(1)) => B; entry: a_e; }
        state B { on(after Duration::from_secs(1)) => A; entry: b_e; }
    }
    }

    impl TimerOnlyActions for TimerOnlyActionContext<'_> {
        fn a_e(&mut self) { self.log.push("a".into()); }
        fn b_e(&mut self) { self.log.push("b".into()); }
    }

    #[test]
    fn t10_2_timer_only() {
        let mut m = TimerOnly::new(TestCtx::default());
        m.step(Duration::ZERO);
        m.step(Duration::from_secs(1));
        m.step(Duration::from_secs(1));
        assert_eq!(m.context().logs(), ["a", "b", "a"]);
    }
}

// ---- T10.3: deeply nested ----
mod t10_3 {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { _U }

    statechart! {
Deep {
        context: TestCtx;
        events: Ev;
        default(A);
        entry: root_e;
        state A {
            default(B);
            entry: a_e;
            state B {
                default(C);
                entry: b_e;
                state C {
                    default(D);
                    entry: c_e;
                    state D {
                        default(E);
                        entry: d_e;
                        state E { entry: e_e; }
                    }
                }
            }
        }
    }
    }

    impl DeepActions for DeepActionContext<'_> {
        fn root_e(&mut self) { self.log.push("root".into()); }
        fn a_e(&mut self) { self.log.push("a".into()); }
        fn b_e(&mut self) { self.log.push("b".into()); }
        fn c_e(&mut self) { self.log.push("c".into()); }
        fn d_e(&mut self) { self.log.push("d".into()); }
        fn e_e(&mut self) { self.log.push("e".into()); }
    }

    #[test]
    fn t10_3_deep_descent() {
        let mut m = Deep::new(TestCtx::default());
        m.step(Duration::ZERO);
        assert_eq!(m.current_state(), DeepState::E);
        assert_eq!(m.context().logs(), ["root", "a", "b", "c", "d", "e"]);
    }
}

// ---- T10.4: chain of emits ----
mod t10_4 {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { Foo, Bar, Baz }

    statechart! {
T104 {
        context: TestCtx;
        events: Ev;
        default(A);
        state A {
            on(Foo) => emit_bar;
            on(Bar) => emit_baz;
            on(Baz) => B;
        }
        state B { entry: b_entry; }
    }
    }

    impl T104Actions for T104ActionContext<'_> {
        fn emit_bar(&mut self) { self.emit(Ev::Bar).unwrap(); self.log.push("bar".into()); }
        fn emit_baz(&mut self) { self.emit(Ev::Baz).unwrap(); self.log.push("baz".into()); }
        fn b_entry(&mut self) { self.log.push("b".into()); }
    }

    #[test]
    fn t10_4_emit_chain() {
        let mut m = T104::new(TestCtx::default());
        m.step(Duration::ZERO);
        m.send(Ev::Foo).unwrap();
        for _ in 0..5 { m.step(Duration::ZERO); }
        assert_eq!(m.current_state(), T104State::B);
        assert_eq!(m.context().logs(), ["bar", "baz", "b"]);
    }
}

// ---- T10.5: emit during termination exit cascade ----
mod t10_5 {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { Halt, Cleanup }

    statechart! {
T105 {
        context: TestCtx;
        events: Ev;
        default(A);
        terminate(Halt);
        on(Cleanup) => do_cleanup;
        state A { exit: a_exit; }
    }
    }

    impl T105Actions for T105ActionContext<'_> {
        fn a_exit(&mut self) {
            self.log.push("a_exit".into());
            // emit should be a no-op / ignored after termination begins
            let _ = self.emit(Ev::Cleanup);
        }
        fn do_cleanup(&mut self) { self.log.push("do_cleanup".into()); }
    }

    #[test]
    fn t10_5_emit_during_term() {
        let mut m = T105::new(TestCtx::default());
        m.step(Duration::ZERO);
        m.send(Ev::Halt).unwrap();
        m.step(Duration::ZERO);
        assert!(m.is_terminated());
        // Keep stepping; do_cleanup should NOT run
        for _ in 0..3 { m.step(Duration::ZERO); }
        assert!(!m.context().logs().contains(&"do_cleanup"));
    }
}

// ---- T10.6: transition from root level ----
mod t10_6 {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { _U }

    statechart! {
T106 {
        context: TestCtx;
        events: Ev;
        default(Deep1);
        entry: root_entry;
        exit: root_exit;
        on(after Duration::from_secs(60)) => FirstState;
        state Deep1 {
            default(Deep2);
            exit: d1_exit;
            state Deep2 { exit: d2_exit; }
        }
        state FirstState { entry: first_entry; }
    }
    }

    impl T106Actions for T106ActionContext<'_> {
        fn root_entry(&mut self) { self.log.push("root_entry".into()); }
        fn root_exit(&mut self) { self.log.push("root_exit".into()); }
        fn d1_exit(&mut self) { self.log.push("d1_exit".into()); }
        fn d2_exit(&mut self) { self.log.push("d2_exit".into()); }
        fn first_entry(&mut self) { self.log.push("first_entry".into()); }
    }

    #[test]
    fn t10_6_root_transition() {
        let mut m = T106::new(TestCtx::default());
        m.step(Duration::ZERO);
        m.context_mut().log.clear();
        m.step(Duration::from_secs(60));
        assert_eq!(m.current_state(), T106State::FirstState);
        // Root is LCA of itself → NOT exited; only Deep1/Deep2 exit
        let logs = m.context().logs();
        assert!(!logs.contains(&"root_exit"));
        assert_eq!(logs, ["d2_exit", "d1_exit", "first_entry"]);
    }
}

// ---- T10.7: into_context after termination ----
mod t10_7 {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { Halt }

    statechart! {
T107 {
        context: TestCtx;
        events: Ev;
        default(A);
        terminate(Halt);
        state A { entry: a_e; }
    }
    }

    impl T107Actions for T107ActionContext<'_> {
        fn a_e(&mut self) { self.log.push("hello".into()); }
    }

    #[test]
    fn t10_7_into_context() {
        let mut m = T107::new(TestCtx::default());
        m.step(Duration::ZERO);
        m.send(Ev::Halt).unwrap();
        m.step(Duration::ZERO);
        assert!(m.is_terminated());
        let ctx = m.into_context();
        assert_eq!(ctx.log, vec!["hello".to_string()]);
    }
}

// ---- T10.1: same action name used in multiple states ----
mod t10_1 {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { Go }

    statechart! {
T101 {
        context: TestCtx;
        events: Ev;
        default(A);
        state A { entry: shared_setup; on(Go) => B; }
        state B { entry: shared_setup; }
    }
    }

    impl T101Actions for T101ActionContext<'_> {
        fn shared_setup(&mut self) { self.log.push("shared".into()); }
    }

    #[test]
    fn t10_1_shared_action() {
        let mut m = T101::new(TestCtx::default());
        m.step(Duration::ZERO);
        m.send(Ev::Go).unwrap();
        m.step(Duration::ZERO);
        assert_eq!(m.context().logs(), ["shared", "shared"]);
    }
}

// ---- T8.1: timer starts on state entry (dedicated per spec §9.8) ----
mod t8_1 {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { _U }

    statechart! {
T81 {
        context: TestCtx;
        events: Ev;
        default(Red);
        state Red { on(after Duration::from_secs(5)) => Green; }
        state Green { entry: green_on; }
    }
    }

    impl T81Actions for T81ActionContext<'_> {
        fn green_on(&mut self) { self.log.push("green_on".into()); }
    }

    #[test]
    fn t8_1_timer_starts_on_entry() {
        let mut m = T81::new(TestCtx::default());
        m.step(Duration::ZERO);
        m.step(Duration::from_millis(4900));
        assert_eq!(m.current_state(), T81State::Red);
        m.step(Duration::from_millis(100));
        assert_eq!(m.current_state(), T81State::Green);
    }
}

// ---- T8.2: timer cancelled on state exit (dedicated per spec §9.8) ----
mod t8_2 {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { Interrupt }

    statechart! {
T82 {
        context: TestCtx;
        events: Ev;
        default(Red);
        state Red {
            on(after Duration::from_secs(5)) => Green;
            on(Interrupt) => Yellow;
        }
        state Green { entry: green_on; }
        state Yellow { entry: yellow_on; }
    }
    }

    impl T82Actions for T82ActionContext<'_> {
        fn green_on(&mut self) { self.log.push("green_on".into()); }
        fn yellow_on(&mut self) { self.log.push("yellow_on".into()); }
    }

    #[test]
    fn t8_2_timer_cancelled_on_exit() {
        let mut m = T82::new(TestCtx::default());
        m.step(Duration::ZERO);
        m.step(Duration::from_secs(2));
        m.send(Ev::Interrupt).unwrap();
        m.step(Duration::ZERO);
        assert_eq!(m.current_state(), T82State::Yellow);
        m.step(Duration::from_secs(5));
        assert_eq!(m.current_state(), T82State::Yellow);
        assert!(!m.context().logs().contains(&"green_on"));
    }
}

// ---- B1: emit_or_panic convenience (§12.1) ----
mod b1_emit_or_panic {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { Foo, Bar }

    statechart! {
B1M {
        context: TestCtx;
        events: Ev;
        default(A);
        state A {
            on(Foo) => emit_bar;
            on(Bar) => B;
        }
        state B { entry: b_entry; }
    }
    }

    impl B1MActions for B1MActionContext<'_> {
        fn emit_bar(&mut self) {
            self.log.push("emit_bar".into());
            self.emit_or_panic(Ev::Bar);
        }
        fn b_entry(&mut self) { self.log.push("b_entry".into()); }
    }

    #[test]
    fn b1_emit_or_panic() {
        let mut m = B1M::new(TestCtx::default());
        m.step(Duration::ZERO);
        m.send(Ev::Foo).unwrap();
        m.step(Duration::ZERO);
        m.step(Duration::ZERO);
        assert_eq!(m.current_state(), B1MState::B);
        assert_eq!(m.context().logs(), ["emit_bar", "b_entry"]);
    }
}

// ---- B2: has_pending_events (§12.3) ----
mod b2_has_pending {
    use super::*;
    #[derive(Debug, Clone)]
    pub enum Ev { Foo }

    statechart! {
B2M {
        context: TestCtx;
        events: Ev;
        default(A);
        state A { on(Foo) => noop; }
    }
    }

    impl B2MActions for B2MActionContext<'_> {
        fn noop(&mut self) { self.log.push("noop".into()); }
    }

    #[test]
    fn b2_has_pending_events() {
        let mut m = B2M::new(TestCtx::default());
        m.step(Duration::ZERO);
        assert!(!m.has_pending_events());
        m.send(Ev::Foo).unwrap();
        assert!(m.has_pending_events());
        m.step(Duration::ZERO);
        assert!(!m.has_pending_events());
    }
}

// ---- B5: optional `events:` for timer-only machines (§12.2) ----
mod b5_no_events {
    use super::*;

    statechart! {
B5M {
        context: TestCtx;
        default(A);
        state A { on(after Duration::from_millis(100)) => B; exit: a_exit; }
        state B { entry: b_entry; }
    }
    }

    impl B5MActions for B5MActionContext<'_> {
        fn a_exit(&mut self) { self.log.push("a_exit".into()); }
        fn b_entry(&mut self) { self.log.push("b_entry".into()); }
    }

    #[test]
    fn b5_timer_only_no_events() {
        let mut m = B5M::new(TestCtx::default());
        m.step(Duration::ZERO);
        assert_eq!(m.current_state(), B5MState::A);
        m.step(Duration::from_millis(100));
        assert_eq!(m.current_state(), B5MState::B);
        assert_eq!(m.context().logs(), ["a_exit", "b_entry"]);
    }
}
