//! Simulated radio task demonstrating `during:` activities.
//!
//! Models a state machine with a couple of I/O loops:
//!
//! * `Idle` waits for a command; no ongoing I/O.
//! * `Receiving` has a `during: next_packet(driver, buf)` that produces a
//!   `PacketRx` event every time the simulated driver yields one.
//! * `Transmitting` has a `during: await_tx_done(driver)` that waits for the
//!   simulated TX to finish.
//!
//! Compared to the "imperative outer loop" pattern (see the dogfood firmware
//! radio task pre-v0.2), the task body collapses to a single
//! `machine.run().await` call — the statechart owns the control flow.
//!
//! Run with: `cargo run --example during_radio --features tokio`

use hsmc::{statechart, Duration};

/// A stand-in for a real LoRa driver. Maintains a simple state so the
/// simulated RX produces deterministic packets.
pub struct SimRadio {
    rx_counter: u32,
    tx_pending: bool,
}

impl Default for SimRadio {
    fn default() -> Self {
        Self::new()
    }
}

impl SimRadio {
    pub fn new() -> Self {
        Self {
            rx_counter: 0,
            tx_pending: false,
        }
    }
    /// Simulates waiting for a packet. Returns Some((rssi, snr)) when one
    /// "arrives". In a real driver this would drive SPI + DIO interrupts.
    pub async fn rx(&mut self, buf: &mut [u8; 64]) -> (i16, i16, usize) {
        tokio::time::sleep(Duration::from_millis(30)).await;
        self.rx_counter = self.rx_counter.wrapping_add(1);
        let n = core::cmp::min(buf.len(), 4);
        buf[..n].copy_from_slice(&self.rx_counter.to_le_bytes()[..n]);
        (-62 + (self.rx_counter as i16 % 10), 8, n)
    }
    /// Starts a transmit. The `await_tx_done` during polls `tx_pending`.
    pub fn begin_tx(&mut self) {
        self.tx_pending = true;
    }
    pub async fn tx_done(&mut self) {
        // In real driver: spin on DIO until TX_DONE.
        tokio::time::sleep(Duration::from_millis(15)).await;
        self.tx_pending = false;
    }
}

pub struct Ctx {
    pub radio: SimRadio,
    pub rx_buf: [u8; 64],
    pub stats: RadioStats,
    pub last_payload: heapless::Vec<u8, 64>,
}

impl Default for Ctx {
    fn default() -> Self {
        Self::new()
    }
}

impl Ctx {
    pub fn new() -> Self {
        Self {
            radio: SimRadio::new(),
            rx_buf: [0; 64],
            stats: RadioStats::default(),
            last_payload: heapless::Vec::new(),
        }
    }
}

#[derive(Default, Debug)]
pub struct RadioStats {
    pub rx_count: u32,
    pub tx_count: u32,
    pub last_rssi: i16,
}

#[derive(Debug, Clone)]
pub enum Ev {
    StartRx,
    StopRx,
    Transmit,
    PacketRx { rssi: i16, snr: i16, len: usize },
    TxDone,
    Halt,
}

statechart! {
Radio {
    context: Ctx;
    events: Ev;
    default(Idle);
    terminate(Halt);

    state Idle {
        entry: announce_idle;
        on(StartRx) => Receiving;
        on(Transmit) => Transmitting;
    }

    state Receiving {
        entry: announce_rx;
        during: next_packet(radio, rx_buf);
        on(PacketRx { rssi: i16, snr: i16, len: usize }) => record_packet;
        on(StopRx) => Idle;
    }

    state Transmitting {
        entry: begin_tx_hw;
        during: await_tx_done(radio);
        on(TxDone) => tx_completed, Idle;
    }
}
}

/// During activity: wait for a packet and return a PacketRx event. Runs
/// every time the machine is in Receiving; dropped whenever the machine
/// leaves Receiving or any other event arrives.
async fn next_packet(radio: &mut SimRadio, buf: &mut [u8; 64]) -> Ev {
    let (rssi, snr, len) = radio.rx(buf).await;
    Ev::PacketRx { rssi, snr, len }
}

/// During activity: wait for the TX to finish. Returns TxDone; then the
/// handler runs and transitions back to Idle.
async fn await_tx_done(radio: &mut SimRadio) -> Ev {
    radio.tx_done().await;
    Ev::TxDone
}

impl RadioActions for RadioActionContext<'_> {
    async fn announce_idle(&mut self) {
        println!("[radio] → Idle");
    }
    async fn announce_rx(&mut self) {
        println!("[radio] → Receiving");
    }
    async fn begin_tx_hw(&mut self) {
        println!("[radio] → Transmitting");
        self.radio.begin_tx();
    }
    async fn record_packet(&mut self, rssi: i16, snr: i16, len: usize) {
        self.stats.rx_count = self.stats.rx_count.wrapping_add(1);
        self.stats.last_rssi = rssi;
        let copy = core::cmp::min(len, self.rx_buf.len());
        let snapshot: heapless::Vec<u8, 64> = self.rx_buf[..copy].iter().copied().collect();
        self.last_payload = snapshot;
        let count = self.stats.rx_count;
        println!("[radio] rx#{} rssi={} snr={} len={}", count, rssi, snr, len);
    }
    async fn tx_completed(&mut self) {
        self.stats.tx_count = self.stats.tx_count.wrapping_add(1);
        println!("[radio] tx#{} done", self.stats.tx_count);
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    println!("\n{}\n", Radio::<8>::STATE_CHART);

    let mut machine = Radio::new(Ctx::new());
    let sender = machine.sender();

    // External driver: issue a sequence of commands.
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(5)).await;
        let _ = sender.send(Ev::StartRx);
        tokio::time::sleep(Duration::from_millis(100)).await;
        let _ = sender.send(Ev::StopRx);
        tokio::time::sleep(Duration::from_millis(10)).await;
        let _ = sender.send(Ev::Transmit);
        tokio::time::sleep(Duration::from_millis(50)).await;
        let _ = sender.send(Ev::StartRx);
        tokio::time::sleep(Duration::from_millis(80)).await;
        let _ = sender.send(Ev::Halt);
    });

    machine.run().await.expect("run failed");
    let ctx = machine.into_context();
    println!(
        "\nfinal stats: rx={} tx={} last_rssi={}",
        ctx.stats.rx_count, ctx.stats.tx_count, ctx.stats.last_rssi
    );
}
