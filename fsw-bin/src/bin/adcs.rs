//! Runs the ADCS task on mainboard v4 with the IMU behind its own device task: ADCS reads the BMX160
//! gyro and magnetometer at 1 Hz through the IMU service and logs each reading over USB (defmt).
//!
//! ```text
//! high-priority executor (SWI_IRQ_0)     thread-mode executor
//! ──────────────────────────────────     ────────────────────
//! imu_task: owns Bmx160Imu  ◀── IMU_REQ ── adcs_task: AdcsTask<ImuClient, _>
//!                           ── ADCS_IMU_REPLY ─▶
//! ```
//!
//! Run with `cargo run --release --bin adcs`, then read the logs with defmt-print.

#![no_std]
#![no_main]

use defmt::info;
use embassy_executor::{InterruptExecutor, Spawner};
use embassy_rp::interrupt;
use embassy_rp::interrupt::{InterruptExt, Priority};
use embassy_sync::channel::Channel;
use embassy_sync::signal::Signal;
use embassy_time::{Delay, Timer};
use panic_probe as _;

use fsw_apps::adcs::MagCalibration;
use fsw_apps::tasks::adcs::AdcsTask;
use fsw_board::hardware::Hardware;
use fsw_board::services::imu::{self, Bmx160Imu, ImuChannel, ImuClient, VecSlot};

/// Requests to the IMU service.
static IMU_REQ: ImuChannel = Channel::new();
/// ADCS's reply slot for IMU reads.
static ADCS_IMU_REPLY: VecSlot = Signal::new();

/// Executor for device tasks. It runs from a software interrupt, so its tasks preempt everything on
/// the thread-mode executor (`main`'s) as soon as they have work.
static EXECUTOR_HIGH: InterruptExecutor = InterruptExecutor::new();

#[interrupt]
unsafe fn SWI_IRQ_0() {
    unsafe { EXECUTOR_HIGH.on_interrupt() }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let r = fsw_board::init();

    spawner
        .spawn(fsw_board::usb_log::usb_logger(r.usb))
        .unwrap();
    // Give the host time to open the USB serial port before logging.
    Timer::after_secs(3).await;

    let Hardware { power, imu } = Hardware::init(r.power, r.i2c0).await;

    interrupt::SWI_IRQ_0.set_priority(Priority::P2);
    let high_spawner = EXECUTOR_HIGH.start(interrupt::SWI_IRQ_0);

    // The IMU moves into its service task, which is now its only owner.
    high_spawner.spawn(imu_task(imu)).unwrap();

    // ADCS gets a client instead of the IMU. `AdcsTask` only needs a `Gyro + Magnetometer`, so it
    // cannot tell the difference.
    let imu_client = ImuClient::new(&IMU_REQ, &ADCS_IMU_REPLY);
    spawner
        .spawn(adcs_task(AdcsTask::new(
            imu_client,
            MagCalibration::NONE,
            Delay,
        )))
        .unwrap();

    // `power` holds the power-line enable pins. Dropping it would switch the lines off, so `main`
    // keeps it and never returns.
    let _power = power;
    core::future::pending::<()>().await;
}

#[embassy_executor::task]
async fn imu_task(imu: Bmx160Imu) {
    imu::run(imu, &IMU_REQ).await
}

#[embassy_executor::task]
async fn adcs_task(mut task: AdcsTask<ImuClient, Delay>) {
    task.run(|r| {
        // The task keeps the field in T; log it in µT, where Earth's field is about 25-65.
        let mag_ut = r.mag.value.map(|b| b * 1e6);
        info!(
            "mag [{=f32}, {=f32}, {=f32}] uT ({}) | gyro [{=f32}, {=f32}, {=f32}] rad/s ({})",
            mag_ut[0],
            mag_ut[1],
            mag_ut[2],
            r.mag.status,
            r.gyro.value[0],
            r.gyro.value[1],
            r.gyro.value[2],
            r.gyro.status,
        );
    })
    .await
}
