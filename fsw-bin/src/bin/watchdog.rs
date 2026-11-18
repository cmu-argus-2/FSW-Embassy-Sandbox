//! Runs the watchdog task on mainboard v4 and logs a heartbeat over USB (defmt), to check on the
//! board that the external watchdog is serviced, and that it resets the MCU when it is not.
//!
//! ```text
//! high-priority executor (SWI_IRQ_0)      thread-mode executor
//! ──────────────────────────────────      ────────────────────
//! watchdog_task: kick + enable at 1 Hz    main: heartbeat (uptime) at 1 Hz
//! ```
//!
//! The watchdog is created first, before USB logging starts, so it is serviced from boot. With
//! [`STOP_KICKING_AFTER_S`] set, the task stops kicking after that many seconds: the board should
//! then reset, and the heartbeat's uptime starts again from 0. Disable the watchdog in a binary
//! before it misses a kick for any other reason (a debugger halt counts).
//!
//! Run with `cargo run --release --bin watchdog`, then read the logs with defmt-print.

#![no_std]
#![no_main]

use defmt::{info, warn};
use embassy_executor::{InterruptExecutor, Spawner};
use embassy_rp::interrupt;
use embassy_rp::interrupt::{InterruptExt, Priority};
use embassy_time::{Delay, Instant, Timer};
use panic_probe as _;

use fsw_apps::tasks::watchdog::{PERIOD_MS, WatchdogTask};
use fsw_board::gpio::Watchdog;

/// Stop kicking after this many seconds, to check that the watchdog resets the board. `None` kicks
/// forever.
const STOP_KICKING_AFTER_S: Option<u32> = None;

/// Executor for the watchdog. It runs from a software interrupt, so the watchdog is serviced even
/// while a thread-mode task is busy.
static EXECUTOR_HIGH: InterruptExecutor = InterruptExecutor::new();

#[interrupt]
unsafe fn SWI_IRQ_0() {
    unsafe { EXECUTOR_HIGH.on_interrupt() }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let r = fsw_board::init();

    // Disconnected until the task's first kick, which then connects it.
    let watchdog = Watchdog::new(r.watchdog, false);
    interrupt::SWI_IRQ_0.set_priority(Priority::P2);
    let high_spawner = EXECUTOR_HIGH.start(interrupt::SWI_IRQ_0);
    high_spawner
        .spawn(watchdog_task(WatchdogTask::new(watchdog, Delay)))
        .unwrap();

    spawner
        .spawn(fsw_board::usb_log::usb_logger(r.usb))
        .unwrap();
    // Give the host time to open the USB serial port before logging.
    Timer::after_secs(3).await;
    info!("boot: watchdog serviced every {=u32} ms", PERIOD_MS);

    loop {
        info!("uptime {=u64} s", Instant::now().as_secs());
        Timer::after_secs(1).await;
    }
}

#[embassy_executor::task]
async fn watchdog_task(mut task: WatchdogTask<Watchdog, Delay>) {
    let Some(stop_after_s) = STOP_KICKING_AFTER_S else {
        task.run(|| info!("watchdog enabled")).await
    };

    let kicks = (stop_after_s * 1000).div_ceil(PERIOD_MS).max(1);
    for _ in 0..kicks {
        if task.step() {
            info!("watchdog enabled");
        }
        Timer::after_millis(PERIOD_MS as u64).await;
    }
    warn!("stopped kicking: the watchdog should reset the board");
}
