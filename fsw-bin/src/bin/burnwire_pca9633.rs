#![no_std]
#![no_main]

use defmt::{error, info};
use embassy_executor::Spawner;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::i2c::{self, I2c, InterruptHandler};
use embassy_rp::peripherals::{I2C0, USB};
use embassy_rp::{Peri, bind_interrupts};
use embassy_time::{Delay, Timer};
use panic_probe as _;

use fsw_lib::drivers::pca9633::PCA9633;

/*
 * Burn Wire Test (Mainboard v4)
 *
 * PCA9633 at 0x60 on I2C0 (SDA0 = GPIO24, SCL0 = GPIO25). Channels 0-2 are the burn
 * wires; channel 3 switches their supply. PERIPH_PWR_EN (GPIO42) must be high first,
 * or the I2C addresses come up wrong.
 *
 * WARNING: this really does fire a burn wire. Disconnect the wires unless you mean to
 * burn one. BURN_STRENGTH matches FSW-mainboard scripts/pca9633_test.py (which writes
 * register value 10, i.e. 245 in driver units); too high a strength can brown out the board.
 */

const PCA9633_ADDR: u8 = 0x60;
const BURN_CHANNEL: u8 = 0; // 0-2 only; channel 3 is the supply switch
const BURN_STRENGTH: u8 = 245;
const BURN_MS: u32 = 1000;
const COUNTDOWN_S: u32 = 10;

bind_interrupts!(struct Irqs {
    I2C0_IRQ => InterruptHandler<I2C0>;
    USBCTRL_IRQ => embassy_rp::usb::InterruptHandler<USB>;
});

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    let _ = spawner.spawn(defmtusb_wrapper(p.USB));
    Timer::after_secs(3).await;

    info!("--- PCA9633 Burn Wire Test (v4, I2C0) ---");

    // Peripheral 3.3V rail, then give the PCA9633 time to boot
    let mut pwr = Output::new(p.PIN_42, Level::High);
    pwr.set_high();
    Timer::after_secs(3).await;

    let i2c = I2c::new_async(p.I2C0, p.PIN_25, p.PIN_24, Irqs, i2c::Config::default());
    let mut burnwires = PCA9633::new(i2c, PCA9633_ADDR);

    match burnwires.init().await {
        Ok(()) => info!("PCA9633 ready at 0x{:02X}, all channels off", PCA9633_ADDR),
        Err(e) => {
            error!("PCA9633 init failed: {:?}", e);
            idle().await;
        }
    }

    for remaining in (1..=COUNTDOWN_S).rev() {
        info!(
            "Firing channel {} in {}s (strength {})",
            BURN_CHANNEL, remaining, BURN_STRENGTH
        );
        Timer::after_secs(1).await;
    }

    info!("Burning channel {} for {}ms", BURN_CHANNEL, BURN_MS);
    match burnwires
        .burn(BURN_CHANNEL, BURN_STRENGTH, BURN_MS, &mut Delay)
        .await
    {
        Ok(()) => info!("Burn complete, driver disabled"),
        Err(e) => error!("Burn failed: {:?} (driver disable was still attempted)", e),
    }

    idle().await;
}

async fn idle() -> ! {
    loop {
        info!("Idle - burn wires off");
        Timer::after_secs(10).await;
    }
}

#[embassy_executor::task]
async fn defmtusb_wrapper(usb: Peri<'static, USB>) {
    let driver = embassy_rp::usb::Driver::new(usb, Irqs);
    let config = {
        let mut c = embassy_usb::Config::new(0x1234, 0x5678);
        c.serial_number = Some("defmt");
        c.max_packet_size_0 = 64;
        c.composite_with_iads = true;
        c.device_class = 0xEF;
        c.device_sub_class = 0x02;
        c.device_protocol = 0x01;
        c
    };
    defmt_embassy_usbserial::run(driver, config).await;
}
