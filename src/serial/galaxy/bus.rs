use log::trace;
use std::{io, time::Duration};
use thiserror::Error;

use super::crc::GalaxyCRC;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_serial::SerialStream;

/// INTERPACKET_GAP is the duration that must be allowed between transmissions on the bus to allow
/// for signal propagation.
const INTERPACKET_GAP: Duration = Duration::from_millis(10);

/// PANEL_ADDRESS is the identifier of the panel on the bus, used as the recipient for all response
/// messages.
const PANEL_ADDRESS: u8 = 0x11;

/// BUS_TIMEOUT is the time after which a read operation for replies from devices gives up.
const BUS_TIMEOUT: Duration = Duration::from_millis(100);

pub struct Bus {
    serial_port: SerialStream,
}

#[derive(Clone, Debug, Error)]
pub enum ReadError {
    #[error("Timed out")]
    Timeout,
    #[error("No data available")]
    NoData,
    #[error("Insufficient data")]
    InsufficientData,
    #[error("CRC check failed")]
    CrcCheckFailed,
    #[error("Reply not addressed to panel: {0}")]
    /// The reply was not found to be addressed to the panel address, but to some other address.
    /// This is likely indicative of a tamper condition.
    InvalidReplyRecipient(u8),
    #[error("IO error: {0}")]
    IoError(std::sync::Arc<io::Error>),
}

impl From<io::Error> for ReadError {
    fn from(value: io::Error) -> Self {
        Self::IoError(std::sync::Arc::new(value))
    }
}

impl Bus {
    pub fn new(serial_port: SerialStream) -> Bus {
        Bus { serial_port }
    }

    pub async fn send_receive_buffered(
        &mut self,
        data: &[u8],
        reply: &mut [u8],
    ) -> Result<usize, ReadError> {
        assert!(
            data.len() >= 2,
            "insufficient data provided to send to Galaxy bus"
        );
        let crc = data.galaxy_crc();

        trace!("output data {:02X?} crc {:02X}", data, crc);

        let mut data = data.to_vec();
        data.push(crc);

        AsyncWriteExt::write_all(&mut self.serial_port, &data)
            .await
            .map_err(ReadError::from)?;
        // AsyncWriteExt::write_u8(&mut self.serial_port, crc)
        //     .await
        //     .map_err(ReadError::from)?;

        tokio::time::sleep(
            INTERPACKET_GAP
                + Duration::from_millis(
                    // 1 stop bit
                    1u64 + 10
                        * (
                            // CRC byte plus data
                            1 + data.len() as u64
                        ),
                ),
        )
        .await;

        tokio::time::timeout(
            BUS_TIMEOUT,
            AsyncReadExt::read(&mut self.serial_port, reply),
        )
        .await
        .map_err(|_| ReadError::Timeout)
        .and_then(|result| match result {
            Ok(bytes_read) => {
                trace!(
                    "response {} bytes: {:02X?}",
                    bytes_read,
                    &reply[0..bytes_read]
                );

                if bytes_read == 0 {
                    return Err(ReadError::NoData);
                } else if bytes_read < 3 {
                    return Err(ReadError::InsufficientData);
                }

                // Check the reply was directed to the panel.
                match reply[0] {
                    PANEL_ADDRESS => (),
                    n => return Err(ReadError::InvalidReplyRecipient(n)),
                };

                let crc = reply[bytes_read - 1];
                if crc != reply[0..bytes_read - 1].galaxy_crc() {
                    return Err(ReadError::CrcCheckFailed);
                }

                // Don't tell the caller about the CRC; the bus handles checking it and returns
                // a better error in case it's missing or invalid.
                Ok(bytes_read - 1)
            }
            Err(e) => Err(ReadError::from(e)),
        })
    }
}
