//! Runs the ADCS task on mainboard v4: reads the BMX160 gyro at 1 Hz and logs each reading over USB
//! (defmt).
//!
//! Run with `cargo run --release --bin adcs`, then read the logs with defmt-print.

#![no_std]
#![no_main]

use defmt::info;
use embassy_executor::Spawner;
use embassy_time::{Delay, Timer};
use panic_probe as _;

use fsw_apps::tasks::adcs::AdcsTask;
use fsw_board::hardware::Hardware;
use fsw_board::imu::Bmx160Imu;

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let r = fsw_board::init();

    spawner
        .spawn(fsw_board::usb_log::usb_logger(r.usb))
        .unwrap();
    // Give the host time to open the USB serial port before logging.
    Timer::after_secs(3).await;

    let Hardware { power, imu } = Hardware::init(r.power, r.i2c0).await;

    spawner.spawn(adcs_task(AdcsTask::new(imu, Delay))).unwrap();

    // `power` holds the power-line enable pins. Dropping it would switch the lines off, so `main`
    // keeps it and never returns.
    let _power = power;
    core::future::pending::<()>().await;
}

#[embassy_executor::task]
async fn adcs_task(mut task: AdcsTask<Bmx160Imu, Delay>) {
    task.run(|r| info!("gyro {} rad/s ({})", r.value, r.status))
        .await
}
