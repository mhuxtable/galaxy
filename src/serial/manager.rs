use derive_more::Display;
use log::{debug, error, trace, warn};
use std::{collections::HashMap, sync::Arc, time::Duration};

use self::queue::BackoffState;

use super::{
    galaxy::{self, bus::ReadError},
    DeliveryError, SerialMessage,
};

/// LAST_MESSAGE_BAD_CHECKSUM_REPLY_COMMAND is the command returned from a device when the last
/// message was corrupted or not understood by the device.
const LAST_MESSAGE_BAD_CHECKSUM_REPLY_COMMAND: u8 = 0xF2;

pub trait SerialDevice: Send + Sync {
    fn next_message(&self) -> (u8, Option<Vec<u8>>);
    fn receive_update(&self, _: Result<SerialMessage, DeliveryError>);
}

#[derive(Clone, Copy, Debug, Display, PartialEq)]
pub enum DeviceStatus {
    Offline,
    OnlineOK,
    OnlineCorruptReplies,
    Unknown,
}

struct DeviceState {
    device: Arc<dyn SerialDevice>,
    status: DeviceStatus,
    failures: u16,
}

impl DeviceState {
    fn new(device: Arc<dyn SerialDevice>) -> DeviceState {
        DeviceState {
            device,
            status: DeviceStatus::Unknown,
            failures: 0,
        }
    }
}

mod queue {
    use std::collections::HashMap;

    /// Backoff is truncated binary exponential backoff, so we'll backoff at most this number of
    /// times (raised to the base) before truncating further backoff.
    const MAX_BACKOFF_CYCLES: usize = 4;

    #[derive(Default)]
    pub struct BackoffState {
        backoff_devices: HashMap<u8, (usize, usize)>, // (current_backoff, max_backoff)
    }

    impl BackoffState {
        pub fn new() -> BackoffState {
            Default::default()
        }

        pub fn mark_device_backoff(&mut self, id: u8) {
            self.backoff_devices
                .entry(id)
                .and_modify(|(current_backoff, max_backoff)| {
                    *max_backoff = (*max_backoff + 1).min(MAX_BACKOFF_CYCLES);
                    *current_backoff = 1 + (1 << *max_backoff);
                })
                .or_insert((2, 0));
        }

        /// Determine whether the provided device is currently in backoff, returning
        /// Some(current_backoff) and decrementing the backoff counter if so. Alternatively, if the
        /// device should not be in backoff, returns None.
        pub fn visit_device(&mut self, id: u8) -> Option<usize> {
            // For each device, when its next backoff becomes 0, it becomes active again, i.e.
            // response is None. If we find its current_backoff already 0, this implies it became
            // active on the previous iteration of the loop and has not been marked as offline and
            // in backoff since; by inference, it's now active again and the backoff record should
            // be removed so that the next time it falls offline, backoff starts from the
            // beginning.

            match self.backoff_devices.get_mut(&id) {
                Some((current_backoff, _)) => {
                    if *current_backoff == 0 {
                        self.backoff_devices.remove(&id);
                        return None;
                    }

                    *current_backoff -= 1;
                    if *current_backoff == 0 {
                        None
                    } else {
                        Some(*current_backoff)
                    }
                }
                None => None,
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use std::collections::VecDeque;

        use super::*;

        #[test]
        fn test_device_not_in_backoff() {
            let mut boff = BackoffState::new();

            assert_eq!(boff.visit_device(3), None);
        }

        #[test]
        fn test_device_in_backoff_then_active() {
            let mut boff = BackoffState::new();

            boff.mark_device_backoff(3);
            assert_eq!(boff.visit_device(3), Some(1));
            assert_eq!(boff.visit_device(3), None);
            // This visit means the device is now effectively active, meaning subsequent marking in
            // backoff should start again at 1 step.
            assert_eq!(boff.visit_device(3), None);

            boff.mark_device_backoff(3);
            assert_eq!(boff.visit_device(3), Some(1));
            assert_eq!(boff.visit_device(3), None);
            // This visit means the device is now effectively active, meaning subsequent marking in
            // backoff should start again at 1 step.
            assert_eq!(boff.visit_device(3), None);
        }

        #[test]
        fn test_device_in_backoff_then_inactive() {
            let mut boff = BackoffState::new();

            boff.mark_device_backoff(3);
            // effective delay is 1 iteration
            assert_eq!(boff.visit_device(3), Some(1));
            assert_eq!(boff.visit_device(3), None);

            // Device is still inactive. Effective delay is 2 iterations
            boff.mark_device_backoff(3);
            assert_eq!(boff.visit_device(3), Some(2));
            assert_eq!(boff.visit_device(3), Some(1));
            assert_eq!(boff.visit_device(3), None);
        }

        #[test]
        fn test_device_max_backoff() {
            let mut boff = BackoffState::new();

            let mut expect = VecDeque::from(vec![1, 2, 4, 8, 16, 16, 16, 16, 16, 16]);

            for i in 0..10 {
                println!("Loop iteration {}", i);
                boff.mark_device_backoff(3);

                for _ in 0..(expect.pop_front().unwrap()) {
                    println!("Visiting device");
                    assert!(boff.visit_device(3).is_some())
                }

                assert_eq!(boff.visit_device(3), None)
            }
        }
    }
}

pub struct SerialManager {
    pub bus: galaxy::Bus,
    devices: HashMap<u8, DeviceState>,
    backoff: BackoffState,
}

impl SerialManager {
    pub fn new(bus: galaxy::Bus) -> SerialManager {
        SerialManager {
            bus,
            devices: HashMap::new(),
            backoff: BackoffState::new(),
        }
    }

    pub fn register_device(&mut self, id: u8, device: Arc<dyn SerialDevice>) {
        if self.devices.contains_key(&id) {
            panic!("attempting to register duplicate serial device {}", id);
        }

        self.devices.insert(id, DeviceState::new(device));
    }

    // TODO return error?
    pub async fn run(&mut self) {
        let mut reply_buf = vec![0u8; 8];
        let device_ids: Vec<u8> = self.devices.keys().cloned().collect();

        loop {
            for id in &device_ids {
                if let Some(_) = self.backoff.visit_device(*id) {
                    // Device is in backoff.
                    continue;
                }

                debug!("Polling device {}", id);

                let result = self.poll_device(*id, &mut reply_buf[..]).await;

                {
                    let state = self.devices.get_mut(id).unwrap();
                    let old_status = state.status;

                    (state.failures, state.status) = match result {
                        Ok(_) => (0, DeviceStatus::OnlineOK),
                        Err(e) => (
                            state.failures + 1,
                            match e {
                                DeliveryError::Timeout | DeliveryError::BusError(_) => {
                                    // If it's a bus error, we can't tell if it's this specific device that's in fault
                                    // condition. It may or may not be online, but there's probably a far bigger issue
                                    // for which the device state tracking is the least of our concerns.
                                    //
                                    // It's marked as Offline for now, but this could be revisited.
                                    DeviceStatus::Offline
                                }
                                DeliveryError::CrcFailed
                                | DeliveryError::DeserialisationError(_) => {
                                    DeviceStatus::OnlineCorruptReplies
                                }
                            },
                        ),
                    };

                    if state.failures == 3 || (state.failures > 0 && state.failures % 10 == 0) {
                        warn!(
                            "Device {} has exhibited {} communications failures",
                            id, state.failures
                        );
                    } else if state.status != old_status {
                        debug!(
                            "Device {} status has changed from {} to {}",
                            id, old_status, state.status
                        );
                    }

                    if state.status == DeviceStatus::Offline {
                        self.backoff.mark_device_backoff(*id);
                    }
                }
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    async fn poll_device(&mut self, id: u8, reply_buf: &mut [u8]) -> Result<(), DeliveryError> {
        let state = self.devices.get_mut(&id).unwrap();

        let (command, data) = state.device.next_message();
        let data = (SerialMessage {
            recipient_address: id,
            command,
            additional_data: data,
        })
        .serialise_without_crc();

        trace!("Device {}: outbound data {:02X?}", id, data.as_slice());

        let mut retries_left = 3;
        let mut reply_status = Err(DeliveryError::Timeout);

        while retries_left > 0 && reply_status.is_err() {
            reply_status = self
                .bus
                .send_receive_buffered(data.as_slice(), reply_buf)
                .await
                .map_err(|e| match e {
                    ReadError::NoData => DeliveryError::Timeout,
                    ReadError::CrcCheckFailed => DeliveryError::CrcFailed,
                    ReadError::InsufficientData => DeliveryError::CrcFailed,
                    e => DeliveryError::BusError(e),
                })
                .and_then(|bytes_read| {
                    // The data was already CRCed when it came off the bus.
                    SerialMessage::deserialise_unchecked(&reply_buf[0..bytes_read])
                        .map_err(DeliveryError::DeserialisationError)
                });

            let should_retry = match reply_status.as_ref() {
                Ok(reply) if reply.command == LAST_MESSAGE_BAD_CHECKSUM_REPLY_COMMAND => {
                    error!("Device {} last outbound message failed checksum", id);
                    true
                }
                Err(e) => {
                    error!("Device {} failed message delivery: {}", id, e);
                    true
                }
                _ => false,
            };

            if should_retry {
                retries_left -= 1;
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }

        state.device.receive_update(reply_status.clone());

        if reply_status.is_err() {
            error!(
                "device {} had message delivery error: {:?}",
                id, reply_status
            );
        } else {
            debug!(
                "device {} has message delivery result {:?}",
                id, reply_status
            );
        }

        reply_status.map(|_| ())
    }
}
