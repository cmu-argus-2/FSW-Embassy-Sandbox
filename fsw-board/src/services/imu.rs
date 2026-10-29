//! IMU service: owns the [`Bmx160Imu`] and serves gyro reads.
//!
//! ```text
//! ADCS task                         IMU service task
//! ─────────                         ────────────────
//! ImuClient::angular_rate()
//!   ├─ send AngularRate{tag, slot} ─▶ receive request
//!   │                                imu.angular_rate().await   (I2C read)
//!   └─ wait on slot ◀─────────────── slot.signal((tag, result))
//! ```

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::{Duration, with_timeout};
use fsw_hal::sensors::Gyro;
use fsw_hal::{SensorFault, Vec3};

use super::Slot;
use crate::imu::Bmx160Imu;

/// Reply slot for a gyro read.
pub type RateSlot = Slot<Result<Vec3, SensorFault>>;

/// What a client can ask the IMU service for. Each request carries the slot to reply into.
pub enum ImuRequest {
    AngularRate { tag: u32, reply: &'static RateSlot },
}

/// Requests waiting for the IMU service. Room for one request per client is enough, since a client
/// waits for each reply before sending again.
pub type ImuChannel = Channel<CriticalSectionRawMutex, ImuRequest, 4>;

/// Own the IMU and answer requests one at a time, forever.
///
/// A binary wraps this in an `#[embassy_executor::task]` (task functions must live where the
/// concrete types are known) and spawns it on whichever executor the IMU should run on.
pub async fn imu_service(mut imu: Bmx160Imu, requests: &'static ImuChannel) -> ! {
    loop {
        match requests.receive().await {
            ImuRequest::AngularRate { tag, reply } => {
                reply.signal((tag, imu.angular_rate().await));
            }
        }
    }
}

/// How long a client waits for a reply before reporting [`SensorFault`]. A gyro read is one short
/// I2C transfer (well under 1 ms), so this only fires if the service is stuck.
pub const REPLY_TIMEOUT: Duration = Duration::from_millis(20);

/// A [`Gyro`] that reads through the IMU service. Each client needs its own reply slot.
pub struct ImuClient {
    requests: &'static ImuChannel,
    reply: &'static RateSlot,
    /// Tag of the last request sent; replies with any other tag are stale.
    tag: u32,
}

impl ImuClient {
    pub const fn new(requests: &'static ImuChannel, reply: &'static RateSlot) -> Self {
        Self {
            requests,
            reply,
            tag: 0,
        }
    }
}

impl Gyro for ImuClient {
    async fn angular_rate(&mut self) -> Result<Vec3, SensorFault> {
        self.tag = self.tag.wrapping_add(1);
        let tag = self.tag;

        self.requests
            .send(ImuRequest::AngularRate {
                tag,
                reply: self.reply,
            })
            .await;

        // Wait for the reply to this request, skipping any late reply to an earlier one.
        let reply = async {
            loop {
                let (t, result) = self.reply.wait().await;
                if t == tag {
                    return result;
                }
            }
        };
        with_timeout(REPLY_TIMEOUT, reply)
            .await
            .unwrap_or(Err(SensorFault))
    }
}
