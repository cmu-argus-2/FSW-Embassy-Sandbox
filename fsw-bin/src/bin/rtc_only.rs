#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::i2c::{self, I2c, InterruptHandler};
use embassy_rp::peripherals::{I2C1, USB};
use embassy_rp::{Peri, bind_interrupts};
use embassy_time::Timer;
use panic_probe as _;

use fsw_lib::drivers::ds3231::DS3231;

bind_interrupts!(struct Irqs {
    I2C1_IRQ => InterruptHandler<I2C1>;
    USBCTRL_IRQ => embassy_rp::usb::InterruptHandler<USB>;
});

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    // USB Logger Setup
    let _ = spawner.spawn(defmtusb_wrapper(p.USB));
    Timer::after_secs(3).await;

    defmt::info!("--- RTC Presentation Test ---");

    // Peripheral Power
    let mut pwr = Output::new(p.PIN_42, Level::High);
    pwr.set_high();
    Timer::after_millis(200).await;

    let i2c_periph = I2c::new_async(p.I2C1, p.PIN_47, p.PIN_46, Irqs, i2c::Config::default());
    let mut rtc = DS3231::new(i2c_periph, 0x68);

    loop {
        match rtc.datetime().await {
            Ok(dt) => {
                // Access fields directly from the DS3231 DateTime struct
                defmt::info!("Time: {}:{}:{}", dt.hour, dt.minute, dt.second);
                defmt::info!("Date: {}-{}-{}", dt.year, dt.month, dt.day);
            }
            Err(e) => {
                defmt::error!("RTC Error: {:?}", e);
            }
        }
        Timer::after_secs(1).await;
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
