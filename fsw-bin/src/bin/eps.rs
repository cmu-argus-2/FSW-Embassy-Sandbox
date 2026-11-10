//! Runs the EPS task on mainboard v4 with each power device behind its own device task: EPS reads
//! the four rail power monitors at 1 Hz and the fuel gauge every 5 s through their services, and
//! logs each reading over USB (defmt).
//!
//! ```text
//! high-priority executor (SWI_IRQ_0)               thread-mode executor
//! ──────────────────────────────────               ────────────────────
//! power_monitor_task (board)  ◀── BOARD_PWR_REQ ──  eps_task: monitors = [PowerMonitorClient; 4]
//! power_monitor_task (gps)    ◀── GPS_PWR_REQ ────            fuel_gauge = FuelGaugeClient
//! power_monitor_task (radio)  ◀── RADIO_PWR_REQ ──
//! power_monitor_task (jetson) ◀── JETSON_PWR_REQ ─
//!                             ── EPS_POWER_REPLY ───▶ (one slot for all four monitors)
//! fuel_gauge_task             ◀── BATTERY_REQ ────
//!                             ── EPS_BATTERY_REPLY ─▶
//! ```
//!
//! RAM use is reported every 10 s as `BENCH` rows: static RAM, the stack's high-water mark and their
//! sum (see `fsw_board::stack`). Filter them with `grep BENCH`.
//!
//! Run with `cargo run --release --bin eps`, then read the logs with defmt-print.

#![no_std]
#![no_main]

use defmt::{info, warn};
use embassy_executor::{InterruptExecutor, Spawner};
use embassy_rp::interrupt;
use embassy_rp::interrupt::{InterruptExt, Priority};
use embassy_sync::channel::Channel;
use embassy_sync::signal::Signal;
use embassy_time::{Delay, Timer};
use panic_probe as _;

use fsw_apps::eps::Rail;
use fsw_apps::tasks::eps::{EpsTask, Readings};
use fsw_board::hardware::Hardware;
use fsw_board::services::clients::{FuelGaugeClient, PowerMonitorClient};
use fsw_board::services::fuel_gauge::{self, Gauge};
use fsw_board::services::power_monitor::{self, Monitor};
use fsw_board::services::{BatteryChannel, BatterySlot, PowerChannel, PowerSlot};
use fsw_board::stack;
use fsw_lib::bench::PORT;

/// How often RAM use is reported.
const RAM_REPORT_SECS: u64 = 10;

/// Power requests, one channel per rail, each served by that rail's monitor.
static BOARD_PWR_REQ: PowerChannel = Channel::new();
static GPS_PWR_REQ: PowerChannel = Channel::new();
static RADIO_PWR_REQ: PowerChannel = Channel::new();
static JETSON_PWR_REQ: PowerChannel = Channel::new();
/// Battery requests, served by the fuel gauge.
static BATTERY_REQ: BatteryChannel = Channel::new();
/// EPS's reply slots, one per reply type: EPS waits for each reply before sending its next request.
static EPS_POWER_REPLY: PowerSlot = Signal::new();
static EPS_BATTERY_REPLY: BatterySlot = Signal::new();

/// Executor for device tasks. It runs from a software interrupt, so its tasks preempt everything on
/// the thread-mode executor (`main`'s) as soon as they have work.
static EXECUTOR_HIGH: InterruptExecutor = InterruptExecutor::new();

#[interrupt]
unsafe fn SWI_IRQ_0() {
    unsafe { EXECUTOR_HIGH.on_interrupt() }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    // First, so the high-water mark covers everything after it.
    stack::paint();

    let r = fsw_board::init();

    spawner
        .spawn(fsw_board::usb_log::usb_logger(r.usb))
        .unwrap();
    spawner.spawn(ram_task()).unwrap();
    // Give the host time to open the USB serial port before logging.
    Timer::after_secs(3).await;

    let Hardware {
        power,
        board_pwr,
        gps_pwr,
        radio_pwr,
        jetson_pwr,
        fuel_gauge,
        ..
    } = Hardware::init(r.power, r.i2c0, r.i2c1).await;

    interrupt::SWI_IRQ_0.set_priority(Priority::P2);
    let high_spawner = EXECUTOR_HIGH.start(interrupt::SWI_IRQ_0);

    // Each device moves into its service task, which is now its only owner.
    for (monitor, requests) in [
        (board_pwr, &BOARD_PWR_REQ),
        (gps_pwr, &GPS_PWR_REQ),
        (radio_pwr, &RADIO_PWR_REQ),
        (jetson_pwr, &JETSON_PWR_REQ),
    ] {
        high_spawner
            .spawn(power_monitor_task(monitor, requests))
            .unwrap();
    }
    high_spawner.spawn(fuel_gauge_task(fuel_gauge)).unwrap();

    // EPS gets one client per rail, in `Rail` order, all sharing one reply slot.
    let monitors = [
        PowerMonitorClient::new(&BOARD_PWR_REQ, &EPS_POWER_REPLY),
        PowerMonitorClient::new(&GPS_PWR_REQ, &EPS_POWER_REPLY),
        PowerMonitorClient::new(&RADIO_PWR_REQ, &EPS_POWER_REPLY),
        PowerMonitorClient::new(&JETSON_PWR_REQ, &EPS_POWER_REPLY),
    ];
    let fuel_gauge = FuelGaugeClient::new(&BATTERY_REQ, &EPS_BATTERY_REPLY);
    spawner
        .spawn(eps_task(EpsTask::new(fuel_gauge, monitors, Delay)))
        .unwrap();

    // `power` holds the power-line enable pins. Dropping it would switch the lines off, so `main`
    // keeps it and never returns.
    let _power = power;
    core::future::pending::<()>().await;
}

#[embassy_executor::task(pool_size = Rail::COUNT)]
async fn power_monitor_task(monitor: Monitor, requests: &'static PowerChannel) {
    power_monitor::run(monitor, requests).await
}

#[embassy_executor::task]
async fn fuel_gauge_task(gauge: Gauge) {
    fuel_gauge::run(gauge, &BATTERY_REQ).await
}

/// Report RAM use: static RAM (fixed), the stack's high-water mark so far, and their sum. The
/// stack peak only grows, so the last report of a run is the run's peak.
#[embassy_executor::task]
async fn ram_task() {
    let static_ram = stack::static_ram();
    loop {
        Timer::after_secs(RAM_REPORT_SECS).await;
        let peak = stack::peak();
        info!(
            "BENCH,{=str},memory,static_ram,eps,total,{=usize},B,1",
            PORT, static_ram
        );
        info!(
            "BENCH,{=str},memory,stack_peak,eps,max,{=usize},B,1",
            PORT, peak
        );
        info!(
            "BENCH,{=str},memory,ram_used,eps,max,{=usize},B,1",
            PORT,
            static_ram + peak
        );
        info!(
            "BENCH,{=str},memory,ram_used,eps,max,{=f32},%,1",
            PORT,
            stack::percent_of_ram(static_ram + peak)
        );
        info!(
            "RAM: {=usize} B static ({=f32} %) + {=usize} B stack peak ({=f32} %) = {=usize} B, {=f32} % of {=usize} KiB",
            static_ram,
            stack::percent_of_ram(static_ram),
            peak,
            stack::percent_of_ram(peak),
            static_ram + peak,
            stack::percent_of_ram(static_ram + peak),
            stack::ram_size() / 1024
        );
    }
}

#[embassy_executor::task]
async fn eps_task(mut task: EpsTask<FuelGaugeClient, PowerMonitorClient, Delay>) {
    task.run(log_readings).await
}

fn log_readings(r: &Readings) {
    for rail in &r.rails {
        match rail.value {
            Some(p) if rail.over_limit => warn!(
                "{} {=f32} V {=f32} A: over power limit",
                rail.rail, p.voltage, p.current
            ),
            Some(p) => info!("{} {=f32} V {=f32} A", rail.rail, p.voltage, p.current),
            None => warn!("{} read failed", rail.rail),
        }
    }
    match r.battery {
        Some(Ok(b)) => info!(
            "battery {=f32} % {=f32} Ah {=f32} V {=f32} A | {=f32} / {=f32} / {=f32} C | flag {}",
            b.soc,
            b.capacity,
            b.voltage,
            b.current,
            b.temperature_ain1,
            b.temperature_ain2,
            b.temperature_die,
            r.flag
        ),
        Some(Err(_)) => warn!("battery read failed | flag {}", r.flag),
        None => {}
    }
}
