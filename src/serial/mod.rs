// Galaxy encapsulates all logic for interfacing with the Galaxy serial bus and devices.
pub mod devices;
pub mod galaxy;
pub mod manager;
mod message;

pub use manager::SerialDevice;
pub use message::{DeliveryError, SerialMessage, SerialResponseResult, SerialTransaction};
