use super::galaxy::crc::galaxy_crc_vectored;
use super::galaxy::{self, GalaxyCRC};

use thiserror::Error;

#[derive(Clone, Debug)]
pub struct SerialMessage {
    pub recipient_address: u8,
    pub command: u8,
    pub additional_data: Option<Vec<u8>>,
}

impl GalaxyCRC for SerialMessage {
    fn galaxy_crc(&self) -> u8 {
        let header = vec![self.recipient_address, self.command];

        let mut vs = vec![header.as_slice()];
        if let Some(additional_data) = self.additional_data.as_ref() {
            vs.push(additional_data);
        }

        galaxy_crc_vectored(&vs)
    }
}

#[derive(Clone, Debug, Error, PartialEq)]
pub enum DeserialisationError {
    #[error("Missing recipient")]
    MissingRecipient,
    #[error("Missing command")]
    MissingCommand,
    #[error("Missing CRC")]
    MissingCrc,
    #[error("CRC failure")]
    CrcFailed,
}

impl SerialMessage {
    pub fn serialise(&self) -> Vec<u8> {
        let mut serialised = self.serialise_without_crc();

        let crc = serialised.galaxy_crc();
        serialised.push(crc);

        serialised
    }

    pub fn serialise_without_crc(&self) -> Vec<u8> {
        let mut serialised = vec![self.recipient_address, self.command];

        if let Some(additional_data) = &self.additional_data {
            serialised.extend_from_slice(additional_data);
        }

        serialised
    }

    pub fn deserialise(data: &[u8]) -> Result<SerialMessage, DeserialisationError> {
        // Do this check first, otherwise deserialise_unchecked might return erroneously that we
        // are missing a command, when in reality it is the CRC that's absent.
        if data.len() == 2 {
            return Err(DeserialisationError::MissingCrc);
        }

        let msg = SerialMessage::deserialise_unchecked(&data[0..data.len() - 1])?;

        // data.len() >= 3 asserted already
        let crc = data[data.len() - 1];

        if msg.galaxy_crc() != crc {
            Err(DeserialisationError::CrcFailed)
        } else {
            Ok(msg)
        }
    }

    /// Blindly deserialises the provided data into a SerialMessage, without checking whether the
    /// CRC passes. This call assumes that the `data` slice does not contain the CRC byte at the
    /// end; if included, it would be naively included in additional_data.
    ///
    /// This will still perform basic checks on the data length to ensure sufficient data is
    /// provided to fill a `SerialMessage`.
    pub fn deserialise_unchecked(data: &[u8]) -> Result<SerialMessage, DeserialisationError> {
        let mut bytes = data.iter();

        let recipient_address = *bytes.next().ok_or(DeserialisationError::MissingRecipient)?;
        let command = *bytes.next().ok_or(DeserialisationError::MissingCommand)?;

        let additional_data: Vec<u8> = bytes.cloned().collect();

        Ok(SerialMessage {
            recipient_address,
            command,
            additional_data: if additional_data.len() > 0 {
                Some(additional_data)
            } else {
                None
            },
        })
    }
}

#[derive(Clone, Debug, Error)]
pub enum DeliveryError {
    #[error("Timeout")]
    Timeout,
    #[error("CRC check failed")]
    CrcFailed,
    #[error("Deserialisation error: {0}")]
    DeserialisationError(#[from] DeserialisationError),
    #[error("Bus error: {0}")]
    BusError(#[from] galaxy::bus::ReadError),
}

pub type SerialResponseResult = Result<SerialMessage, DeliveryError>;

#[derive(Debug)]
pub struct SerialTransaction {
    pub command: u8,
    pub data: Option<Vec<u8>>,
    pub response_channel: tokio::sync::mpsc::Sender<SerialResponseResult>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialise_success() {
        let data = vec![0x10, 0x20, 0x30, 0x40, 0x4B];
        let result = SerialMessage::deserialise(&data);

        assert!(result.is_ok());
        let msg = result.unwrap();
        assert_eq!(msg.recipient_address, 0x10);
        assert_eq!(msg.command, 0x20);
        assert_eq!(msg.additional_data.unwrap(), vec![0x30, 0x40]);
    }

    #[test]
    fn test_deserialise_crc_failure() {
        let data = vec![0x10, 0x20, 0x30, 0x40, 0xAB]; // Incorrect CRC
        let result = SerialMessage::deserialise(&data);

        assert!(result.is_err());
        assert_eq!(result.err(), Some(DeserialisationError::CrcFailed));
    }

    #[test]
    fn test_deserialise_missing_fields() {
        let data = vec![0x10, 0x20]; // Missing additional data and CRC
        let result = SerialMessage::deserialise(&data);

        assert!(result.is_err());
        assert_eq!(result.err(), Some(DeserialisationError::MissingCrc));
    }

    #[test]
    fn test_deserialise_unchecked_success() {
        let data = vec![0x10, 0x20, 0x30, 0x40]; // No CRC
        let result = SerialMessage::deserialise_unchecked(&data);

        assert!(result.is_ok());
        let msg = result.unwrap();
        assert_eq!(msg.recipient_address, 0x10);
        assert_eq!(msg.command, 0x20);
        assert_eq!(msg.additional_data.unwrap(), vec![0x30, 0x40]);
    }

    #[test]
    fn test_deserialise_unchecked_missing_fields() {
        let data = vec![0x10, 0x20]; // Missing additional data
        let result = SerialMessage::deserialise_unchecked(&data);

        assert!(result.is_ok());
        let msg = result.unwrap();
        assert_eq!(msg.recipient_address, 0x10);
        assert_eq!(msg.command, 0x20);
        assert!(msg.additional_data.is_none());
    }

    #[test]
    fn test_deserialise_unchecked_no_additional_data() {
        let data = vec![0x01, 0x02]; // No additional data
        let result = SerialMessage::deserialise_unchecked(&data);

        assert_eq!(result.unwrap().additional_data, None);
    }

    #[test]
    fn test_serialise_no_additional_data() {
        let msg = SerialMessage {
            recipient_address: 0x01,
            command: 0x02,
            additional_data: None,
        };

        let expected_output = vec![0x01, 0x02, 0xAD];

        assert_eq!(msg.serialise(), expected_output);
    }

    #[test]
    fn test_serialise_with_additional_data() {
        let msg = SerialMessage {
            recipient_address: 0x01,
            command: 0x02,
            additional_data: Some(vec![0x03, 0x04]),
        };

        let expected_output = vec![0x01, 0x02, 0x03, 0x04, 0xB4];

        assert_eq!(msg.serialise(), expected_output);
    }

    #[test]
    fn test_serialise_without_crc_no_additional_data() {
        let msg = SerialMessage {
            recipient_address: 0x01,
            command: 0x02,
            additional_data: None,
        };

        let expected_output = vec![0x01, 0x02];

        assert_eq!(msg.serialise_without_crc(), expected_output);
    }

    #[test]
    fn test_serialise_without_crc_with_additional_data() {
        let msg = SerialMessage {
            recipient_address: 0x01,
            command: 0x02,
            additional_data: Some(vec![0x03, 0x04]),
        };

        let expected_output = vec![0x01, 0x02, 0x03, 0x04];

        assert_eq!(msg.serialise_without_crc(), expected_output);
    }
}
