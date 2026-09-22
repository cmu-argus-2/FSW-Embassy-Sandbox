#![no_std]
#![no_main]

use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_rp::gpio::{Level, Output};
use embassy_time::Timer;
use panic_probe as _;

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    // SAFE MODE: Use internal oscillator (ROSC) instead of external crystal
    let p = embassy_rp::init(Default::default());

    // GP0 (NEOPIXEL data) - Mainboard v4 has no plain LED, and GP25 is SCL0
    let mut led = Output::new(p.PIN_0, Level::Low);

    loop {
        led.set_high();
        Timer::after_millis(100).await;
        led.set_low();
        Timer::after_millis(100).await;
    }
}
