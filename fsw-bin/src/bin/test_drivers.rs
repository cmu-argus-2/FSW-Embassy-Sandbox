//! This example shows how to share (async) I2C and SPI buses between multiple devices.

#![no_std]
#![no_main]

use defmt::{info, error};
use embassy_embedded_hal::shared_bus::asynch::i2c::I2cDevice;
use embassy_executor::Spawner;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::i2c::{self, I2c, InterruptHandler};
use embassy_rp::peripherals::{I2C0, I2C1, USB};

use embassy_rp::{Peri, bind_interrupts};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::Timer;
use static_cell::StaticCell;
use {panic_probe as _};

use vl53l4cd_ulp::VL53L4cd;
use vl53l4cd_ulp::Error;

use rtt_target::rtt_init_print;

//use package name given in Cargo.toml
use fsw_lib::drivers::adm1176::ADM1176 as adm1176;
use fsw_lib::drivers::vl53l4cd::VL53L4CD;

type I2c0Bus = Mutex<NoopRawMutex, I2c<'static, I2C0, i2c::Async>>;
type I2c1Bus = Mutex<NoopRawMutex, I2c<'static, I2C1, i2c::Async>>;

bind_interrupts!(struct Irqs {
    I2C0_IRQ => InterruptHandler<I2C0>;
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
    // Allow the switched peripheral supply to settle before I2C access.
    Timer::after_millis(100).await;

    let sda = p.PIN_46;
    let scl = p.PIN_47;

    // Shared I2C bus
    let i2c = I2c::new_async(p.I2C1, scl, sda, Irqs, i2c::Config::default());
    
    static I2C_BUS: StaticCell<I2c1Bus> = StaticCell::new();
    let i2c1_bus = I2C_BUS.init(Mutex::new(i2c));

    // Additional shared bus: I2C0, SDA = GPIO24, SCL = GPIO25.
    let i2c0 = I2c::new_async(p.I2C0, p.PIN_25, p.PIN_24, Irqs, i2c::Config::default());
    static I2C0_BUS: StaticCell<I2c0Bus> = StaticCell::new();
    let _i2c0_bus = I2C0_BUS.init(Mutex::new(i2c0));

    //spawn adm1176 driver task
    // spawner.spawn(adm1176_task(i2c1_bus));
    // Deployment sensor: I2C1, SDA = GPIO46, SCL = GPIO47.
    spawner.spawn(vl53l4cd_task(i2c1_bus));

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
    // This backend otherwise waits for a full buffer before transmitting.
    // Its USB writer polls every 100 ms; flush less often to let it drain.
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

#[embassy_executor::task]
async fn adm1176_task(i2c_bus: &'static I2c1Bus) {
    let i2c_dev = I2cDevice::new(i2c_bus);
    let mut sensor = adm1176::new(i2c_dev, 0x40);
    sensor.config(&["V_CONT", "I_CONT"]).await;
    loop {
        match sensor.read_voltage_current().await {
            Ok((voltage, current)) => {
                info!("voltage {}, current {}", voltage, current);
            }
            Err(e) => {
                error!("{}", e);
            }
        }
        
        Timer::after_secs(1).await;
    }
}

#[embassy_executor::task]
async fn vl53l4cd_task(i2c_bus: &'static I2c1Bus) {
    let i2c_dev = I2cDevice::new(i2c_bus);
    let mut sensor = VL53L4CD::new(i2c_dev, 0x29, embassy_time::Delay);
    if let Err(e) = sensor.init().await {
        error!("VL53L4CD initialization failed: {:?}", e);
        return;
    }
    loop {
        match sensor.read_distance().await {
            Ok(Some(distance)) => {
                info!("Distance: {} mm", distance);
            }
            Ok(None) => {}
            Err(e) => {
                error!("VL53L4CD read failed: {:?}", e);
            }
        }
        Timer::after_secs(1).await;
    } 
}
