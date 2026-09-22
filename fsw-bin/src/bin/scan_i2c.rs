//! This example shows how to share (async) I2C and SPI buses between multiple devices.

#![no_std]
#![no_main]

use defmt::info;
use embassy_executor::Spawner;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::i2c::{self, I2c, InterruptHandler};
use embassy_rp::peripherals::{I2C0, I2C1, USB};
use embassy_rp::{Peri, bind_interrupts};
use embassy_time::Timer;
use {panic_probe as _};

use rtt_target::rtt_init_print;

bind_interrupts!(struct Irqs {
    I2C1_IRQ => InterruptHandler<I2C1>;
    I2C0_IRQ => InterruptHandler<I2C0>;
    USBCTRL_IRQ => embassy_rp::usb::InterruptHandler<embassy_rp::peripherals::USB>;
});

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    info!("Here we go!");

    //set up global logger
    rtt_init_print!();

    spawner.spawn(defmtusb_wrapper(p.USB));

    Timer::after_secs(3).await;

    info!("I2C Setup");

    //turn on peri
    let mut led = Output::new(p.PIN_42, Level::Low);
    led.set_high();
    // Allow the switched peripheral supply to settle before I2C access.
    Timer::after_millis(100).await;

    // Shared I2C bus
    let mut i2c0 = I2c::new_async(p.I2C0, p.PIN_25, p.PIN_24, Irqs, i2c::Config::default());
    let mut i2c1 = I2c::new_async(p.I2C1, p.PIN_47, p.PIN_46, Irqs, i2c::Config::default());

    info!("Starting I2C0");
    let mut buf = [0u8];
    for address in 0..=127u8 {
        // i2c.write returns OK on every address, so use i2c.read to detect device
        match i2c0.read_async(address, &mut buf).await {
            Ok(res) => {
                info!("Found device at address: {:#X} with result: {:?}", address, res);
            },
            Err(_) => {
                // info!("No device at address: {:#X}\r\n", address);
                //address not found, do nothing
            },
        }
        //delay needed to prevent overloading i2c bus
        Timer::after_millis(10).await;
    }
    info!("Starting I2C1");
    let mut buf = [0u8];
    for address in 0..=127u8 {
        // i2c.write returns OK on every address, so use i2c.read to detect device
        match i2c1.read_async(address, &mut buf).await {
            Ok(res) => {
                info!("Found device at address: {:#X} with result: {:?}", address, res);
            },
            Err(_) => {
                // info!("No device at address: {:#X}\r\n", address);
                //address not found, do nothing
            },
        }
        //delay needed to prevent overloading i2c bus
        Timer::after_millis(10).await;
    }
    loop {
        info!("looping...");
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
    let flush_logs = async {
        loop {
            Timer::after_millis(250).await;
            defmt::flush();
        }
    };
    embassy_futures::join::join(
        defmt_embassy_usbserial::run(driver, config),
        flush_logs,
    ).await;
}
