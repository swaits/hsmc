//! Interactive microwave oven TUI driven by `hsmc`.
//!
//! Run it:
//!
//! ```text
//! cargo run --example microwave_tui --features tokio
//! ```
//!
//! Keys:
//!   S  — Start cook              O — Open door
//!   X  — Stop / cancel           C — Close door
//!   K  — Simulate power failure (terminates the machine)
//!   Q  — Quit
//!
//! This demonstrates the same statechart as `microwave.rs` but hooked up to
//! live keyboard input and a real-time ratatui UI. The state machine runs
//! on `oven.run()` in one task; a second task reads keys and forwards them
//! via a cloned `sender()`; a third task redraws the screen 30×/sec using
//! state published by entry/exit actions.

use std::{
    collections::VecDeque,
    io,
    sync::{Arc, Mutex},
    time::Instant,
};

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture, Event, EventStream, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt;
use hsmc::{statechart, Duration};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph, Wrap},
    Terminal,
};

// ---------- Shared UI state ----------
//
// The actions write into this through the user context. The render task
// reads it. `Arc<Mutex<_>>` keeps it cheap and thread-safe.

#[derive(Default)]
pub struct UiState {
    state_path: Vec<&'static str>,
    display: String,
    magnetron: bool,
    heater: bool,
    door_open: bool,
    blink: bool,
    log: VecDeque<String>,
    /// Instant at which the cook should complete. Advances by the pause
    /// duration on resume so the progress bar freezes while paused.
    cook_deadline: Option<Instant>,
    cook_total: Duration,
    /// Set when we enter Paused; cleared on resume. Freezes the gauge.
    paused_at: Option<Instant>,
    terminated: bool,
}

impl UiState {
    async fn log(&mut self, msg: impl Into<String>) {
        self.log.push_back(msg.into());
        while self.log.len() > 50 {
            self.log.pop_front();
        }
    }
    async fn set_path(&mut self, path: &[&'static str]) {
        self.state_path = path.to_vec();
    }
}

// ---------- User context ----------

pub struct OvenContext {
    pub ui: Arc<Mutex<UiState>>,
}

// ---------- Events ----------

#[derive(Debug, Clone)]
pub enum OvenEvent {
    StartPressed,
    StopPressed,
    DoorOpened,
    DoorClosed,
    Reset,
    /// Sent by the external watchdog when the adjusted cook deadline is met.
    /// Using an event instead of a static `Duration` transition lets us pause
    /// and resume the cook clock — something hsmc timers can't do on their own.
    CookDone,
    PowerFailure,
}

// ---------- Statechart ----------
//
// Same shape as microwave.rs (every feature exercised) with a longer cook
// time so the user has time to interact. The cook timer lives on `Running`
// so it survives Heating⇄Paused transitions.

const COOK_MS: u64 = 8_000;

statechart! {
Oven {
    context: OvenContext;
    events: OvenEvent;

    entry: boot_self_test;
    entry: log_boot_done;
    exit: save_last_state;
    exit: cut_all_power;
    default(Idle);
    terminate(PowerFailure);

    // Root-level door tracking. Any state whose local handler shadows these
    // must repeat `mark_door_*` alongside its own handlers.
    on(DoorOpened) => mark_door_open;
    on(DoorClosed) => mark_door_closed;

    // Idle is split so Start can only fire when the door is closed. This is
    // the HSM equivalent of a guard — a structural check rather than a
    // boolean predicate.
    state Idle {
        // `sync_door` runs after the default descent completes handling, then
        // emits DoorOpened if the door is physically open. That re-aligns the
        // machine with reality when we land in Idle from Running (e.g. Stop
        // pressed while door is open). The emitted event is processed after
        // the entry cascade, so DoorClosed → DoorOpen happens cleanly.
        entry: enter_idle, sync_door;
        default(DoorClosed);

        state DoorClosed {
            entry: show_ready;
            // Local handlers, so we must repeat mark_door_open here.
            on(DoorOpened) => mark_door_open;
            on(DoorOpened) => DoorOpen;

            on(StartPressed) => record_start;
            on(StartPressed) => prime_cook;
            on(StartPressed) => Running;
        }

        state DoorOpen {
            entry: show_close_door;
            on(DoorClosed) => mark_door_closed;
            on(DoorClosed) => DoorClosed;
            // Pressing Start while door is open: beep + log, no transition.
            on(StartPressed) => warn_door_open;
        }
    }

    state Running {
        entry: enter_running, start_cook_clock;
        exit: stop_cook_clock;
        default(Cooking);

        on(after Duration::from_millis(250)) => heartbeat;
        on(StopPressed) => Idle;
        on(CookDone) => Done;

        state Cooking {
            entry: enter_cooking, show_cooking;
            default(Heating);

            state Heating {
                // Magnetron + heater live on Heating so they turn off the
                // instant we leave for Paused.
                entry: enter_heating, start_magnetron, heater_on;
                exit: heater_off, stop_magnetron;
                on(DoorOpened) => mark_door_open;
                on(DoorOpened) => pause_cook_clock;
                on(DoorOpened) => Suspended;
            }

            // Suspended = "cook is paused". Two substates express the classic
            // microwave rule: closing the door alone does NOT resume cooking
            // — the user must press Start. Pressing Stop (handled by Running)
            // cancels entirely back to Idle.
            state Suspended {
                entry: enter_suspended, start_pause_blink;
                exit: stop_pause_blink;
                default(DoorAjar);

                state DoorAjar {
                    entry: show_paused;
                    on(DoorClosed) => mark_door_closed;
                    on(DoorClosed) => AwaitingResume;
                }

                state AwaitingResume {
                    entry: show_press_start;
                    on(DoorOpened) => mark_door_open;
                    on(DoorOpened) => DoorAjar;

                    // Actions fire before the transition, so the clock is
                    // resumed and the log line written before we re-enter
                    // Heating (which turns the magnetron back on).
                    on(StartPressed) => log_resume;
                    on(StartPressed) => resume_cook_clock;
                    on(StartPressed) => Heating;
                }
            }
        }
    }

    state Done {
        entry: enter_done, beep, show_done;
        exit: exit_done;

        on(after Duration::from_millis(2000)) => schedule_auto_reset;
        on(StartPressed) => Done;
        on(Reset) => Idle;
    }
}
}

// ---------- Actions ----------
//
// Every method is either publishing to the shared UI state or emitting an
// event back into the machine.

impl OvenActions for OvenActionContext<'_> {
    // Root
    async fn boot_self_test(&mut self) { self.ui.lock().unwrap().log("BOOT self-test ok"); }
    async fn log_boot_done(&mut self)  { self.ui.lock().unwrap().log("BOOT ready"); }
    async fn save_last_state(&mut self){ self.ui.lock().unwrap().log("SHUTDOWN persisted state"); }
    async fn cut_all_power(&mut self) {
        let mut ui = self.ui.lock().unwrap();
        ui.magnetron = false;
        ui.heater = false;
        ui.display = "-- OFF --".into();
        ui.terminated = true;
        ui.log("SHUTDOWN power cut");
    }

    // Door tracking (root-level)
    async fn mark_door_open(&mut self) {
        let mut ui = self.ui.lock().unwrap();
        ui.door_open = true;
    }
    async fn mark_door_closed(&mut self) {
        let mut ui = self.ui.lock().unwrap();
        ui.door_open = false;
    }

    // Idle
    async fn enter_idle(&mut self) {
        let mut ui = self.ui.lock().unwrap();
        ui.set_path(&["Oven", "Idle"]);
    }
    async fn sync_door(&mut self) {
        let door_open = self.ui.lock().unwrap().door_open;
        if door_open {
            self.ui.lock().unwrap().log("IDLE door still open — syncing");
            let _ = self.emit(OvenEvent::DoorOpened);
        }
    }
    async fn show_ready(&mut self) {
        let mut ui = self.ui.lock().unwrap();
        ui.set_path(&["Oven", "Idle", "DoorClosed"]);
        ui.display = "READY".into();
        ui.log("IDLE ready");
    }
    async fn show_close_door(&mut self) {
        let mut ui = self.ui.lock().unwrap();
        ui.set_path(&["Oven", "Idle", "DoorOpen"]);
        ui.display = "CLOSE DOOR".into();
        ui.log("IDLE door is open");
    }
    async fn warn_door_open(&mut self) {
        self.ui.lock().unwrap().log("WARN can't start — door open");
    }
    async fn record_start(&mut self) { self.ui.lock().unwrap().log("BTN start pressed"); }
    async fn prime_cook(&mut self)   { self.ui.lock().unwrap().log("BTN primed cook"); }
    async fn beep(&mut self)         { self.ui.lock().unwrap().log("*BEEP*"); }

    // Running
    async fn enter_running(&mut self) {
        let mut ui = self.ui.lock().unwrap();
        ui.set_path(&["Oven", "Running"]);
    }
    async fn start_magnetron(&mut self) {
        let mut ui = self.ui.lock().unwrap();
        ui.magnetron = true;
        ui.log("MAG on");
    }
    async fn stop_magnetron(&mut self) {
        let mut ui = self.ui.lock().unwrap();
        ui.magnetron = false;
        ui.log("MAG off");
    }
    async fn start_cook_clock(&mut self) {
        let mut ui = self.ui.lock().unwrap();
        let total = Duration::from_millis(COOK_MS);
        ui.cook_total = total;
        ui.cook_deadline = Some(Instant::now() + total);
        ui.paused_at = None;
    }
    async fn stop_cook_clock(&mut self) {
        let mut ui = self.ui.lock().unwrap();
        ui.cook_deadline = None;
        ui.paused_at = None;
    }
    async fn pause_cook_clock(&mut self) {
        let mut ui = self.ui.lock().unwrap();
        ui.paused_at = Some(Instant::now());
        ui.log("CLOCK paused");
    }
    async fn resume_cook_clock(&mut self) {
        let mut ui = self.ui.lock().unwrap();
        if let (Some(paused_at), Some(deadline)) = (ui.paused_at, ui.cook_deadline) {
            let skipped = paused_at.elapsed();
            ui.cook_deadline = Some(deadline + skipped);
            ui.log(format!("CLOCK resume (+{}ms)", skipped.as_millis()));
        }
        ui.paused_at = None;
    }
    async fn heartbeat(&mut self) { /* UI gauge animates on its own */ }

    // Cooking / Heating
    async fn enter_cooking(&mut self) {
        let mut ui = self.ui.lock().unwrap();
        ui.set_path(&["Oven", "Running", "Cooking"]);
    }
    async fn show_cooking(&mut self) {
        let mut ui = self.ui.lock().unwrap();
        ui.display = "COOKING".into();
        ui.blink = false;
    }
    async fn enter_heating(&mut self) {
        let mut ui = self.ui.lock().unwrap();
        ui.set_path(&["Oven", "Running", "Cooking", "Heating"]);
        ui.blink = false;
        ui.display = "COOKING".into();
    }
    async fn heater_on(&mut self) {
        let mut ui = self.ui.lock().unwrap();
        ui.heater = true;
        ui.log("HEAT on");
    }
    async fn heater_off(&mut self) {
        let mut ui = self.ui.lock().unwrap();
        ui.heater = false;
        ui.log("HEAT off");
    }

    // Suspended (paused)
    async fn enter_suspended(&mut self) {
        let mut ui = self.ui.lock().unwrap();
        ui.set_path(&["Oven", "Running", "Cooking", "Suspended"]);
    }
    async fn show_paused(&mut self) {
        let mut ui = self.ui.lock().unwrap();
        ui.set_path(&["Oven", "Running", "Cooking", "Suspended", "DoorAjar"]);
        ui.display = "PAUSED".into();
    }
    async fn show_press_start(&mut self) {
        let mut ui = self.ui.lock().unwrap();
        ui.set_path(&["Oven", "Running", "Cooking", "Suspended", "AwaitingResume"]);
        ui.display = "PRESS START".into();
        ui.log("PAUSE door closed — press Start to resume");
    }
    async fn start_pause_blink(&mut self) {
        let mut ui = self.ui.lock().unwrap();
        ui.blink = true;
        ui.log("PAUSE blink on");
    }
    async fn stop_pause_blink(&mut self) {
        let mut ui = self.ui.lock().unwrap();
        ui.blink = false;
        ui.log("PAUSE blink off");
    }
    async fn log_resume(&mut self) { self.ui.lock().unwrap().log("PAUSE resuming"); }

    // Done
    async fn enter_done(&mut self) {
        let mut ui = self.ui.lock().unwrap();
        ui.set_path(&["Oven", "Done"]);
    }
    async fn show_done(&mut self) {
        self.ui.lock().unwrap().display = "DONE!".into();
    }
    async fn exit_done(&mut self) { self.ui.lock().unwrap().log("DONE exit"); }
    async fn schedule_auto_reset(&mut self) {
        self.ui.lock().unwrap().log("DONE auto-reset queued");
        self.emit(OvenEvent::Reset).expect("queue has room");
    }
}

// ---------- Rendering ----------

fn draw(frame: &mut ratatui::Frame<'_>, ui: &UiState) {
    let size = frame.area();
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(16),
            Constraint::Length(10),
            Constraint::Length(3),
        ])
        .split(size);

    // --- Title bar ---
    let title = if ui.terminated {
        Line::from(vec![
            Span::styled(" MICROWAVE ", Style::new().bg(Color::Red).fg(Color::White).bold()),
            Span::raw("  "),
            Span::styled("TERMINATED", Style::new().fg(Color::Red).bold()),
        ])
    } else {
        Line::from(vec![
            Span::styled(" MICROWAVE ", Style::new().bg(Color::Yellow).fg(Color::Black).bold()),
            Span::raw("  hsmc statechart demo · press Q to quit"),
        ])
    };
    frame.render_widget(Paragraph::new(title), root[0]);

    // --- Main area: oven on the left, status on the right ---
    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(root[1]);

    draw_oven(frame, main[0], ui);
    draw_status(frame, main[1], ui);

    // --- Log ---
    let log_lines: Vec<Line> = ui.log.iter().rev().take(8).rev()
        .map(|s| Line::from(s.as_str()))
        .collect();
    let log = Paragraph::new(log_lines)
        .block(Block::default().title(" Event log ").borders(Borders::ALL))
        .wrap(Wrap { trim: false });
    frame.render_widget(log, root[2]);

    // --- Keys ---
    let keys = Line::from(vec![
        key_hint("S", "Start"),
        Span::raw("  "),
        key_hint("X", "Stop"),
        Span::raw("  "),
        key_hint("O", "Open door"),
        Span::raw("  "),
        key_hint("C", "Close door"),
        Span::raw("  "),
        key_hint("K", "Kill (power fail)"),
        Span::raw("  "),
        key_hint("Q", "Quit"),
    ]);
    frame.render_widget(
        Paragraph::new(keys).block(Block::default().borders(Borders::ALL).title(" Controls ")),
        root[3],
    );
}

fn key_hint(k: &str, label: &str) -> Span<'static> {
    Span::from(format!("[{}] {}", k, label)).fg(Color::Cyan)
}

fn draw_oven(frame: &mut ratatui::Frame<'_>, area: Rect, ui: &UiState) {
    let block = Block::default().borders(Borders::ALL).title(" Oven ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(5),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Min(0),
        ])
        .split(inner);

    // Door indicator line
    let door_line = if ui.door_open {
        Span::styled("  ┌─────  DOOR OPEN  ─────┐", Style::new().fg(Color::Yellow).bold())
    } else {
        Span::styled("  ┌─────  door closed  ─────┐", Style::new().fg(Color::DarkGray))
    };
    frame.render_widget(Paragraph::new(Line::from(door_line)), rows[0]);

    // Big display
    let display_style = if ui.terminated {
        Style::new().fg(Color::Red).bold()
    } else if ui.blink && (Instant::now().elapsed().as_millis() / 300) % 2 == 0 {
        Style::new().fg(Color::Yellow).bold()
    } else if ui.display == "DONE!" {
        Style::new().fg(Color::Green).bold()
    } else if ui.display == "COOKING" {
        Style::new().fg(Color::LightRed).bold()
    } else {
        Style::new().fg(Color::Cyan).bold()
    };
    let big = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled(
            format!(" ┃  {:^14}  ┃", ui.display),
            display_style,
        )),
        Line::from(Span::styled(" ┃                  ┃", Style::new().fg(Color::DarkGray))),
        Line::from(Span::styled(" ┗━━━━━━━━━━━━━━━━━━┛", Style::new().fg(Color::DarkGray))),
    ]);
    frame.render_widget(big, rows[1]);

    // Progress gauge for cook — freezes while paused.
    let (ratio, label, color) = if let Some(deadline) = ui.cook_deadline {
        let now = ui.paused_at.unwrap_or_else(Instant::now);
        let total = ui.cook_total;
        let remaining = deadline.saturating_duration_since(now);
        let elapsed = total.saturating_sub(remaining);
        let r = (elapsed.as_secs_f64() / total.as_secs_f64()).clamp(0.0, 1.0);
        let label = format!("{:.1}s / {:.1}s{}",
            elapsed.as_secs_f64(),
            total.as_secs_f64(),
            if ui.paused_at.is_some() { "  (paused)" } else { "" },
        );
        let color = if ui.paused_at.is_some() { Color::Yellow } else { Color::LightRed };
        (r, label, color)
    } else {
        (0.0, "—".to_string(), Color::DarkGray)
    };
    let gauge = Gauge::default()
        .block(Block::default().borders(Borders::NONE))
        .gauge_style(Style::new().fg(color))
        .ratio(ratio)
        .label(label);
    frame.render_widget(gauge, rows[3]);
}

fn draw_status(frame: &mut ratatui::Frame<'_>, area: Rect, ui: &UiState) {
    let block = Block::default().borders(Borders::ALL).title(" Status ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let path = if ui.state_path.is_empty() {
        "—".to_string()
    } else {
        ui.state_path.join(" › ")
    };

    let bullet = |on: bool| {
        if on { Span::styled("●", Style::new().fg(Color::Green).bold()) }
        else  { Span::styled("○", Style::new().fg(Color::DarkGray)) }
    };

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  State path:  ", Style::new().fg(Color::DarkGray)),
            Span::styled(path, Style::new().fg(Color::White).bold()),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::raw("  Magnetron   "),
            bullet(ui.magnetron),
            Span::raw(if ui.magnetron { "  ON " } else { "  off" }),
        ]),
        Line::from(vec![
            Span::raw("  Heater      "),
            bullet(ui.heater),
            Span::raw(if ui.heater { "  ON " } else { "  off" }),
        ]),
        Line::from(vec![
            Span::raw("  Door        "),
            if ui.door_open {
                Span::styled("OPEN  ", Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD))
            } else {
                Span::styled("CLOSED", Style::new().fg(Color::DarkGray))
            },
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  hsmc tip: ", Style::new().fg(Color::DarkGray)),
            Span::raw("events bubble · timers don't"),
        ]),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

// ---------- Main ----------

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    // Shared UI state.
    let ui = Arc::new(Mutex::new(UiState::default()));

    // Build the machine.
    let ctx = OvenContext { ui: Arc::clone(&ui) };
    let mut oven = Oven::with_queue_capacity::<16>(ctx);
    let sender = oven.sender();
    let fault_sender = oven.sender();
    let cook_sender = oven.sender();

    // Raw-mode terminal.
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Render loop — 30 fps.
    let ui_for_render = Arc::clone(&ui);
    let (render_stop_tx, mut render_stop_rx) = tokio::sync::oneshot::channel::<()>();
    let render_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(33));
        loop {
            tokio::select! {
                _ = &mut render_stop_rx => break,
                _ = interval.tick() => {
                    let snapshot = {
                        let g = ui_for_render.lock().unwrap();
                        UiSnapshot::from(&*g)
                    };
                    let _ = terminal.draw(|f| draw(f, &snapshot.into()));
                }
            }
        }
        // Restore terminal.
        let _ = disable_raw_mode();
        let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture);
        terminal
    });

    // Cook-clock watchdog. Polls shared UI state; when we're not paused and
    // the deadline has passed, fires a single CookDone event and then sleeps
    // until the deadline is re-armed. The machine discards CookDone if the
    // current state doesn't handle it (e.g. after StopPressed).
    let ui_for_watchdog = Arc::clone(&ui);
    let watchdog_task = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let should_fire = {
                let u = ui_for_watchdog.lock().unwrap();
                if u.terminated { return; }
                match (u.cook_deadline, u.paused_at) {
                    (Some(deadline), None) => Instant::now() >= deadline,
                    _ => false,
                }
            };
            if should_fire {
                let _ = cook_sender.send(OvenEvent::CookDone);
                // Give the machine a beat to process and clear cook_deadline
                // (via Running's exit → stop_cook_clock) before we check again.
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    });

    // Input loop.
    let ui_for_input = Arc::clone(&ui);
    let input_task = tokio::spawn(async move {
        let mut events = EventStream::new();
        while let Some(Ok(ev)) = events.next().await {
            if let Event::Key(k) = ev {
                if k.kind != KeyEventKind::Press { continue; }
                let code = k.code;
                let msg = match code {
                    KeyCode::Char('s') | KeyCode::Char('S') => Some(OvenEvent::StartPressed),
                    KeyCode::Char('x') | KeyCode::Char('X') => Some(OvenEvent::StopPressed),
                    KeyCode::Char('o') | KeyCode::Char('O') => Some(OvenEvent::DoorOpened),
                    KeyCode::Char('c') | KeyCode::Char('C') => Some(OvenEvent::DoorClosed),
                    KeyCode::Char('k') | KeyCode::Char('K') => Some(OvenEvent::PowerFailure),
                    KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
                        // Quit: cleanly terminate the machine via PowerFailure.
                        let _ = fault_sender.send(OvenEvent::PowerFailure);
                        break;
                    }
                    _ => None,
                };
                if let Some(m) = msg {
                    ui_for_input.lock().unwrap().log(format!("KEY -> {:?}", m));
                    let _ = sender.send(m);
                }
            }
        }
    });

    // Drive the state machine on the main task.
    let _ = oven.run().await;

    // Let the UI show the TERMINATED banner briefly.
    tokio::time::sleep(Duration::from_millis(700)).await;

    // Stop the render loop.
    let _ = render_stop_tx.send(());
    input_task.abort();
    watchdog_task.abort();
    let mut terminal = render_task.await.expect("render task");

    // Belt-and-braces teardown in case the render loop didn't get to it.
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture);

    // Final transcript.
    let ui = ui.lock().unwrap();
    println!("\nFinal state: {}", ui.state_path.join(" › "));
    println!("Events: {}", ui.log.len());
    for line in ui.log.iter() {
        println!("  {}", line);
    }
    Ok(())
}

// Snapshot so we don't hold the Mutex across the render.
struct UiSnapshot {
    state_path: Vec<&'static str>,
    display: String,
    magnetron: bool,
    heater: bool,
    door_open: bool,
    blink: bool,
    log: VecDeque<String>,
    cook_deadline: Option<Instant>,
    cook_total: Duration,
    paused_at: Option<Instant>,
    terminated: bool,
}
impl From<&UiState> for UiSnapshot {
    fn from(u: &UiState) -> Self {
        Self {
            state_path: u.state_path.clone(),
            display: u.display.clone(),
            magnetron: u.magnetron,
            heater: u.heater,
            door_open: u.door_open,
            blink: u.blink,
            log: u.log.clone(),
            cook_deadline: u.cook_deadline,
            cook_total: u.cook_total,
            paused_at: u.paused_at,
            terminated: u.terminated,
        }
    }
}
impl From<UiSnapshot> for UiState {
    fn from(s: UiSnapshot) -> Self {
        Self {
            state_path: s.state_path,
            display: s.display,
            magnetron: s.magnetron,
            heater: s.heater,
            door_open: s.door_open,
            blink: s.blink,
            log: s.log,
            cook_deadline: s.cook_deadline,
            cook_total: s.cook_total,
            paused_at: s.paused_at,
            terminated: s.terminated,
        }
    }
}
