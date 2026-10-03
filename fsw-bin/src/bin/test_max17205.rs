//! This example shows how to share (async) I2C and SPI buses between multiple devices.

#![no_std]
#![no_main]

use defmt::{error, info};
use embassy_embedded_hal::shared_bus::asynch::i2c::I2cDevice;
use embassy_executor::Spawner;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::i2c::{self, I2c, InterruptHandler};
use embassy_rp::peripherals::{I2C1, USB};

use embassy_rp::{Peri, bind_interrupts};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::Timer;
use panic_probe as _;
use static_cell::StaticCell;

use rtt_target::rtt_init_print;

//use package name given in Cargo.toml
use fsw_lib::drivers::max17205::MAX17205 as max17205;

// MAX17205 I2C addresses: main registers (read) and shadow RAM (write)
const MAX17205_READ_ADDR: u8 = 0x36;
const MAX17205_WRITE_ADDR: u8 = 0x0B;

type I2c1Bus = Mutex<NoopRawMutex, I2c<'static, I2C1, i2c::Async>>;

bind_interrupts!(struct Irqs {
    I2C1_IRQ => InterruptHandler<I2C1>;
    USBCTRL_IRQ => embassy_rp::usb::InterruptHandler<embassy_rp::peripherals::USB>;
});

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    info!("Here we go!");

    //set up global logger
    rtt_init_print!();

    spawner.spawn(defmtusb_wrapper(p.USB));

    //delay needed to set up usb connection
    Timer::after_secs(3).await;

    //turn on peri
    let mut led = Output::new(p.PIN_42, Level::Low);
    led.set_high();

    // Shared I2C bus
    let i2c = I2c::new_async(p.I2C1, p.PIN_47, p.PIN_46, Irqs, i2c::Config::default());
    static I2C_BUS: StaticCell<I2c1Bus> = StaticCell::new();
    let i2c_bus = I2C_BUS.init(Mutex::new(i2c));

    //spawn max17205 driver task
    spawner.spawn(i2c_task_a(i2c_bus));

    loop {
        info!("looping...");
        Timer::after_secs(5).await;
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

#[embassy_executor::task]
async fn i2c_task_a(i2c_bus: &'static I2c1Bus) {
    let i2c_dev = I2cDevice::new(i2c_bus);
    let mut sensor = max17205::new(i2c_dev, MAX17205_READ_ADDR, MAX17205_WRITE_ADDR);
    loop {
        match sensor.read_soc().await {
            Ok(soc) => info!("soc {} %", soc),
            Err(e) => error!("soc: {}", e),
        }
        match sensor.read_capacity().await {
            Ok(capacity) => info!("capacity {} mAh", capacity),
            Err(e) => error!("capacity: {}", e),
        }
        match sensor.read_current().await {
            Ok(current) => info!("current {} mA", current),
            Err(e) => error!("current: {}", e),
        }
        match sensor.read_voltage().await {
            Ok(voltage) => info!("voltage {} mV", voltage),
            Err(e) => error!("voltage: {}", e),
        }
        match sensor.read_midvoltage().await {
            Ok(midvoltage) => info!("midvoltage {} mV", midvoltage),
            Err(e) => error!("midvoltage: {}", e),
        }
        match sensor.read_cycles().await {
            Ok(cycles) => info!("cycles {}", cycles),
            Err(e) => error!("cycles: {}", e),
        }
        match sensor.read_tte().await {
            Ok(tte) => info!("time to empty {} s", tte),
            Err(e) => error!("tte: {}", e),
        }
        match sensor.read_ttf().await {
            Ok(ttf) => info!("time to full {} s", ttf),
            Err(e) => error!("ttf: {}", e),
        }
        match sensor.read_time_pwrup().await {
            Ok(time) => info!("time since power up {}", time),
            Err(e) => error!("time_pwrup: {}", e),
        }
        match sensor.read_temperature().await {
            Ok(temp) => info!("temperature {}", temp),
            Err(e) => error!("temperature: {}", e),
        }
        match sensor.read_temperature_ain1().await {
            Ok(temp) => info!("temperature ain1 {}", temp),
            Err(e) => error!("temperature_ain1: {}", e),
        }
        match sensor.read_temperature_ain2().await {
            Ok(temp) => info!("temperature ain2 {}", temp),
            Err(e) => error!("temperature_ain2: {}", e),
        }
        match sensor.read_temperature_die().await {
            Ok(temp) => info!("temperature die {}", temp),
            Err(e) => error!("temperature_die: {}", e),
        }

        Timer::after_secs(5).await;
    }
}
