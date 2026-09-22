#![no_std]
#![no_main]

use defmt::{error, info, warn};
use embassy_executor::Spawner;
use embassy_rp::Peri;
use embassy_rp::bind_interrupts;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::i2c::{self, I2c, InterruptHandler};
use embassy_rp::peripherals::{I2C0, USB};
use embassy_time::Timer;
use panic_probe as _;

use fsw_lib::drivers::opt4003::OPT4003;

bind_interrupts!(struct Irqs {
    I2C0_IRQ => InterruptHandler<I2C0>;
    USBCTRL_IRQ => embassy_rp::usb::InterruptHandler<USB>;
});

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    // USB Logger Setup
    let _ = spawner.spawn(defmtusb_wrapper(p.USB));
    Timer::after_secs(3).await; // Give USB time to connect

    info!("--- OPT4003 Lux Sensor Test ---");

    // Peripheral Power
    let _pwr = Output::new(p.PIN_42, Level::High);
    Timer::after_millis(500).await;

    // I2C0 on GP25 (SCL0) and GP24 (SDA0) - Mainboard v4
    let i2c = I2c::new_async(p.I2C0, p.PIN_25, p.PIN_24, Irqs, i2c::Config::default());
    let mut sensor = OPT4003::new(i2c, 0x44);

    // Address scanning loop
    'scan: loop {
        for &addr in &[0x44u8, 0x45, 0x46, 0x47] {
            sensor.set_addr(addr);
            if sensor.init().await.is_ok() {
                info!("OPT4003 found at 0x{:02X}", addr);
                break 'scan;
            }
        }
        info!("Searching for OPT4003...");
        Timer::after_millis(500).await;
    }

    loop {
        match sensor.lux().await {
            Ok(lux) => {
                let whole = lux as u32;
                let frac = ((lux - whole as f32) * 100.0) as u32;
                info!("Lux: {}.{:02}", whole, frac);
            }
            Err(e) => {
                warn!("lux failed: {:?}, reinitialising", e);
                if sensor.init().await.is_err() {
                    error!("re-init failed, resetting system");
                    cortex_m::peripheral::SCB::sys_reset();
                }
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
