use log::{debug, error, info};
use std::sync::{Mutex, RwLock};
use std::time::Duration;
use thiserror::Error;
use tokio::sync::mpsc;

use crate::serial::{DeliveryError, SerialDevice, SerialMessage, SerialTransaction};

// KEYS represents the individual keys on the keypad, with the indices representing the code used
// to convey key meaning from the device.
const KEYS: &str = "0123456789BAEX*#";

fn key_to_char(idx: u8) -> char {
    KEYS.chars()
        .nth(idx as usize)
        .expect(format!("key index out of bounds: {:02X}", idx).as_str())
}

#[derive(Clone, Debug)]
pub struct State {
    pub backlight: Backlight,       // LCD backlight
    pub beeper: Beeper,             // keypad sounder
    pub key_clicks: KeyClicks,      // chirp when pressing keys
    pub screen: KeypadDisplayState, // contents of the display
}

impl Default for State {
    fn default() -> Self {
        State {
            backlight: Backlight::Off,
            beeper: Beeper::Off,
            key_clicks: KeyClicks::Off,
            screen: KeypadDisplayState::default(),
        }
    }
}

/// A toggleable flag that returns either the constant B or 0x0, and is toggled each time it is
/// queried. This is used in updates to the keypad to acknowledge events or convey updates with
/// a determination of whether the ack/update is fresh or repeated.
struct UpdateFlag<const B: u8>(bool);

impl<const B: u8> UpdateFlag<B> {
    fn get_toggle(&mut self) -> u8 {
        self.0 ^= true;

        if self.0 {
            // it was false, now it's true
            0x0
        } else {
            B
        }
    }
}

struct KeypadUpdates {
    send_key_ack: bool,

    send_backlight: bool,
    send_beeper: bool,
    send_key_clicks: bool,
    send_screen: bool,

    // Flags used when conveying acknowledgements or update events to the keypad that require an
    // indication of freshness, rather than a repeat of a previous event. Toggled each time they
    // are sent.
    screen_update_flag: UpdateFlag<0x80>,
    key_update_flag: UpdateFlag<0x02>,
}

impl Default for KeypadUpdates {
    fn default() -> Self {
        KeypadUpdates {
            send_key_ack: false,
            send_backlight: false,
            send_beeper: false,
            send_key_clicks: false,
            send_screen: false,

            screen_update_flag: UpdateFlag(true),
            key_update_flag: UpdateFlag(true),
        }
    }
}

/// SerialKeypad handles the serial interface and state management for interacting with a CP-037 or
/// CP-038 keypad on the Galaxy bus. In the case of CP-038, this specifically focuses on the keypad
/// itself; the integrated Prox reader, operating on a distinct serial bus address, is not taken
/// into account in this model.
pub struct SerialKeypad {
    state: RwLock<State>,
    // The keypad is online if last_state is Some.
    last_state: RwLock<Option<State>>,
    tamper: Mutex<bool>,
    // The serial update logic typically works out the correct message to send, but in some cases
    // it is necessary to force sending.
    updates: Mutex<KeypadUpdates>,

    update_ch: mpsc::Sender<Result<SerialMessage, DeliveryError>>,
    monitor_ch: tokio::sync::Mutex<mpsc::Receiver<Result<SerialMessage, DeliveryError>>>,
}

impl Default for SerialKeypad {
    fn default() -> Self {
        let (sender, recv) = mpsc::channel(10);

        Self {
            state: RwLock::new(State::default()),
            last_state: RwLock::new(None),
            tamper: Mutex::new(false),
            updates: Mutex::new(KeypadUpdates::default()),

            update_ch: sender,
            monitor_ch: tokio::sync::Mutex::new(recv),
        }
    }
}

macro_rules! validate_additional_data {
    ($reply:expr, $len:expr) => {
        if let Some(ref data) = $reply.additional_data {
            if data.len() == $len {
                // Proceed with the data.
                true
            } else {
                error!("Received invalid additional data length from keypad");
                false
            }
        } else {
            error!("Received invalid initialisation data from keypad");
            false
        }
    };
}

impl SerialKeypad {
    pub fn new() -> SerialKeypad {
        Default::default()
    }

    pub fn mutate_state<F>(&self, f: F)
    where
        F: FnOnce(&mut State),
    {
        let mut state = self
            .state
            .write()
            .expect("unable to lock keypad state for writing");
        f(&mut state);
    }

    pub fn is_tamper(&self) -> bool {
        *self.tamper.lock().unwrap()
    }

    pub async fn update_worker(&self) {
        let mut ch = self.monitor_ch.lock().await;

        while let Some(msg) = ch.recv().await {
            debug!("got update: {:?}", msg);

            let mut last_state = self
                .last_state
                .write()
                .expect("unable to lock last_state for writing");

            let mut tamper = self
                .tamper
                .lock()
                .expect("unable to lock tamper for writing");

            match msg {
                Ok(reply) => {
                    match ReplyCommand::try_from(reply.command) {
                        Ok(ReplyCommand::Initialised) => {
                            if last_state.is_some() {
                                // TODO figure out how to handle an unexpected init
                                error!("Received initialise response for an already initialised keypad");
                            } else if !validate_additional_data!(reply, 3) {
                                error!("Received invalid initialisation data from keypad");
                            } else {
                                let data = reply.additional_data.unwrap();

                                if (data[0], data[1], data[2]) == (0x08, 0x00, 0x64) {
                                    info!("Keypad initialised");
                                    // Cloning current state does not race with external updates to
                                    // this state, as updates are forced. Normally in the steady
                                    // state this would be unsafe as a write could race and prevent
                                    // sending an update to the pad.
                                    *last_state = Some(self.state.read().unwrap().clone());
                                    *tamper = false;

                                    let mut updates =
                                        self.updates.lock().expect("unable to lock update struct");

                                    updates.send_backlight = true;
                                    updates.send_beeper = true;
                                    updates.send_key_clicks = true;
                                    updates.send_screen = true;
                                }
                            }
                        }
                        Ok(ReplyCommand::Ack) => {
                            *tamper = false;
                        }
                        Ok(ReplyCommand::AckWithKey) => {
                            if !validate_additional_data!(reply, 1) {
                                error!("Received AckWithKey with invalid data length");
                                continue;
                            }

                            let data = reply.additional_data.unwrap()[0];
                            let mut updates = self.updates.lock().unwrap();

                            if data == 0x7F {
                                *tamper = true;
                            } else {
                                *tamper = data & 0x40 == 0x40;

                                let key_press = key_to_char(data & 0xF);
                                info!("Received key press of {} from keypad", key_press);

                                updates.send_key_ack = true;
                            }
                        }
                        Ok(ReplyCommand::BadChecksum) => {
                            error!("Got BadChecksum from device in response to last update");
                            // Device marked as offline.
                            *last_state = None;
                        }
                        Err(_) => {
                            error!("Received unknown reply command {}", reply.command);
                        }
                    }
                }
                Err(_) => {
                    // On error, the device is marked as offline and needs to be reinitialised.
                    *last_state = None;
                }
            }
        }
    }

    fn next_command(&self) -> (Command, Option<Vec<u8>>) {
        let current_state = self.state.read().unwrap();
        let mut last_state_lock = self
            .last_state
            .write()
            .expect("unable to lock keypad last state for writing");

        // unwrap guaranteed to succeed, as the device is guaranteed online
        let last_state = last_state_lock.as_mut().unwrap();
        let mut updates = self.updates.lock().unwrap();

        // We can reset and update current state optimistically when the command is issued. The
        // underlying serial bus is responsible for reliable delivery and replay where necessary.
        // If unsatisfactory responses are received, the entire state is reset, at which point the
        // partial optimistic updates here are immaterial.

        if updates.send_backlight || current_state.backlight != last_state.backlight {
            updates.send_backlight = false;
            last_state.backlight = current_state.backlight;

            (
                Command::Backlight,
                Some(vec![current_state.backlight.into()]),
            )
        } else if updates.send_beeper || current_state.beeper != last_state.beeper {
            updates.send_beeper = false;
            last_state.beeper = current_state.beeper;

            (Command::Beeper, Some(current_state.beeper.into()))
        } else if updates.send_key_clicks || current_state.key_clicks != last_state.key_clicks {
            updates.send_key_clicks = false;
            last_state.key_clicks = current_state.key_clicks;

            (
                Command::KeyClicks,
                Some(vec![current_state.key_clicks.into()]),
            )
        } else if updates.send_screen || current_state.screen != last_state.screen {
            updates.send_screen = false;

            let screen = &current_state.screen;
            last_state.screen = screen.clone();

            // TODO investigate what 0x1F command does in screen updates

            let display_flags = 0x01
                | updates.screen_update_flag.get_toggle()
                | if updates.send_key_ack {
                    updates.send_key_ack = false;
                    0x10 | updates.key_update_flag.get_toggle()
                } else {
                    0
                }
                | if screen.blink { 0x08 } else { 0 };

            let mut data = vec![
                display_flags,
                ScreenOpCodes::DISPLAY_RESET,
                ScreenOpCodes::CURSOR_FIRST_LINE,
                ScreenOpCodes::CURSOR_HIDDEN,
            ];
            data.extend(format!("{:<16}", screen.lines[0]).chars().map(|x| x as u8));
            data.push(ScreenOpCodes::CURSOR_SECOND_LINE);
            data.extend(format!("{:<16}", screen.lines[1]).chars().map(|x| x as u8));

            if let Some(cursor_style) = match screen.cursor_style {
                CursorStyle::Block => Some(ScreenOpCodes::CURSOR_BLOCK_STYLE),
                CursorStyle::Underline => Some(ScreenOpCodes::CURSOR_UNDERLINE_STYLE),
                _ => None,
            } {
                data.push(cursor_style);
            }

            data.extend([ScreenOpCodes::CURSOR_SEEK_BYTE, screen.cursor_position]);

            (Command::Screen, Some(data))
        } else if updates.send_key_ack {
            updates.send_key_ack = false;

            (
                Command::ButtonAck,
                Some(vec![updates.key_update_flag.get_toggle()]),
            )
        } else {
            (Command::Ping, None)
        }
    }
}

impl SerialDevice for SerialKeypad {
    fn next_message(&self) -> SerialTransaction {
        let (command, data) = if self
            .last_state
            .read()
            .expect("unable to read last_state")
            .is_none()
        {
            (Command::Initialise, Some(vec![0x0E]))
        } else {
            self.next_command()
        };

        SerialTransaction {
            command: command.into(),
            data,
            response_channel: self.update_ch.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    // Initialises the keypad in the system from its initial startup state, or if it dropped off
    // the bus for a period of time. Data byte meaning is unknown.
    //
    // 10 00 0E C8
    Initialise,
    // General poll of the device state, used when nothing else needs to be sent.
    //
    // 10 06 01
    Ping,
    // Updates the contents of the screen (partially or fully). Contains subcommands; too complex
    // to give a comprehensive example.
    Screen,
    // Acknowledges receipt of the last button press
    //
    // 10 0B 02 C7. Byte 3 toggles between 0x00 and 0x02 to guard against replays.
    ButtonAck,
    // Sets the keypad beep behaviour from the internal sounder
    //
    // 10 0C 03 02 F0 BC.
    //
    // Byte 3:    0x00 = off, 0x01 = on, 0x03 = intermittent
    // Bytes 4/5: on time, off time in 1/10ths of seconds
    Beeper,
    // Sets state of the device LCD screen backlight
    //
    // 10 0D 01 D4. 0x01 = on, 0x00 = off
    Backlight,
    // Controls clicks from the sounder when a key is pressed
    //
    // 10 19 01 D4. 0x01 = normal, 0x03 = off, 0x05 = quiet
    KeyClicks,
}

impl From<Command> for u8 {
    fn from(value: Command) -> Self {
        match value {
            Command::Initialise => 0x00,
            Command::Ping => 0x06,
            Command::Screen => 0x07,
            Command::KeyClicks => 0x19,
            Command::ButtonAck => 0x0B,
            Command::Beeper => 0x0C,
            Command::Backlight => 0x0D,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplyCommand {
    // Returned after the keypad is initialised. Carries unknown data, possibly the firmware
    // version? 11 FF 08 00 64 28
    Initialised,
    // Acknowledge last message when device state is quiescent with no other state updates to
    // convey. 11 FE BA
    Ack,
    // Acknowledge last message, and convey additional data from keypad (tamper and/or key press).
    // 11 F4 41 F1.
    //
    // Byte 3. 0x7F - tamper only.
    //         0x40 - tamper + key. Keys conveyed in lower nibble.
    AckWithKey,
    // Indicates the keypad could not process the last message.
    // TODO if this is the same for other devices, move it down to the generic galaxy bus.
    BadChecksum,
}

#[derive(Clone, Debug, Error)]
#[error("invalid keypad reply command op code {0}")]
pub struct InvalidReplyCommandByteError(pub u8);

impl TryFrom<u8> for ReplyCommand {
    type Error = InvalidReplyCommandByteError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0xF2 => Ok(Self::BadChecksum),
            0xF4 => Ok(Self::AckWithKey),
            0xFE => Ok(Self::Ack),
            0xFF => Ok(Self::Initialised),
            x => Err(InvalidReplyCommandByteError(x)),
        }
    }
}

impl From<ReplyCommand> for u8 {
    fn from(value: ReplyCommand) -> Self {
        match value {
            ReplyCommand::Initialised => 0xFF,
            ReplyCommand::Ack => 0xFE,
            ReplyCommand::AckWithKey => 0xF4,
            ReplyCommand::BadChecksum => 0xF2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum KeyClicks {
    Off,
    Quiet,
    Normal,
}

impl From<KeyClicks> for u8 {
    fn from(value: KeyClicks) -> Self {
        match value {
            KeyClicks::Off => 0x03,
            KeyClicks::Quiet => 0x05,
            KeyClicks::Normal => 0x01,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Backlight {
    Off,
    On,
}

impl From<Backlight> for u8 {
    fn from(value: Backlight) -> Self {
        match value {
            Backlight::Off => 0x00,
            Backlight::On => 0x01,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Beeper {
    Off,
    On,
    Intermittent { on_time: u8, off_time: u8 },
}

impl Beeper {
    pub fn new_intermittent(on_time: Duration, off_time: Duration) -> Beeper {
        assert!(on_time.as_millis() <= 0xFF * 100);
        assert!(off_time.as_millis() <= 0xFF * 100);

        Beeper::Intermittent {
            on_time: (on_time.as_millis() / 100) as u8,
            off_time: (off_time.as_millis() / 100) as u8,
        }
    }
}

impl From<Beeper> for Vec<u8> {
    fn from(value: Beeper) -> Self {
        match value {
            Beeper::Off => vec![0x00, 0x00, 0x00],
            Beeper::On => vec![0x01, 0x00, 0x00],
            Beeper::Intermittent { on_time, off_time } => vec![0x03, on_time, off_time],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CursorStyle {
    None,
    Block,
    Underline,
}

impl From<CursorStyle> for u8 {
    fn from(value: CursorStyle) -> Self {
        match value {
            CursorStyle::None => 0x07,
            CursorStyle::Block => 0x06,
            CursorStyle::Underline => 0x10,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct KeypadDisplayState {
    pub lines: [String; 2],
    pub cursor_position: u8,
    pub cursor_style: CursorStyle,
    /// LED blink status, i.e. whether it is flashing or not. Odd to be part of the display
    /// updates, but that's where the command to control this is represented.
    pub blink: bool,
}

impl Default for KeypadDisplayState {
    fn default() -> Self {
        KeypadDisplayState {
            lines: [
                String::from("    ********    "),
                String::from("Panel booting up"),
            ],
            cursor_position: 0x0,
            cursor_style: CursorStyle::None,
            blink: false,
        }
    }
}

pub struct ScreenOpCodes;

impl ScreenOpCodes {
    // Cursor Positioning Operations
    pub const CURSOR_FIRST_LINE: u8 = 0x01;
    pub const CURSOR_SECOND_LINE: u8 = 0x02;
    pub const CURSOR_SEEK_BYTE: u8 = 0x03;
    pub const CURSOR_LEFT_NO_ERASE: u8 = 0x15;
    pub const CURSOR_RIGHT_NO_ERASE: u8 = 0x16;

    // Scroll Operations
    pub const SCROLL_LEFT: u8 = 0x04;
    pub const SCROLL_RIGHT: u8 = 0x05;

    // Cursor Style Operations
    pub const CURSOR_BLOCK_STYLE: u8 = 0x06;
    pub const CURSOR_HIDDEN: u8 = 0x07;
    pub const CURSOR_UNDERLINE_STYLE: u8 = 0x10;

    // Text Manipulation
    pub const BACKSPACE: u8 = 0x14;

    // Display Operations
    pub const DISPLAY_RESET: u8 = 0x17;
    pub const FLASH_DISPLAY: u8 = 0x18;
    pub const STOP_FLASHING: u8 = 0x19;

    // 0x08 seems to print an uppercase A-ring (Å) and advance the cursor.
    // 0x09 seems to print a lowercase
    // 0xA6 to 0xAF appear to be symbols of another alphabet/script.
    // 0XB1 to 0xDA are another script.
    // 0xDC to 0xDE are another script.
    // 0xE0 to 0xEF is assorted script, including a-umlaut, some low Greek, and integral
    // 0xF0 to 0xFC is mostly more Greek.

    // Special Characters
    pub const RIGHT_ARROW: u8 = 0x7E; // normal tilde in ASCII
    pub const LEFT_ARROW: u8 = 0x7F; // normally unprintable DEL in ASCII
    pub const WHITESPACE: u8 = 0xA0;
    pub const LOWER_OPEN_FULL_STOP: u8 = 0xA1;
    pub const TOP_LEFT_CORNER: u8 = 0xA2; // similar to Unicode U+231C (⌜)
    pub const BOTTOM_RIGHT_CORNER: u8 = 0xA3; // similar to Unicode U+231F (⌟)
    pub const UNFILLED_BASELINE_SQUARE_UNFILLED: u8 = 0xA4; // similar to Unicode U+25AB (▫)
    pub const SQUARE_BULLET: u8 = 0xA5; // i.e. small filled mid-aligned square, Unicode U+25AA (▪)
    pub const EN_DASH: u8 = 0xB0;
    pub const LARGE_FULL_HEIGHT_SQUARE: u8 = 0xDB; // similar to U+2610 (☐)
    pub const DEGREE_SYMBOL: u8 = 0xDF;
    pub const DIVISION: u8 = 0xFD; // similar to U+00F7 (÷)
    pub const WHITESPACE2: u8 = 0xFE; // similar to ASCII 0xFF?
    pub const SQUARE_LARGE_FULL_FILLED: u8 = 0xFF; // similar to ASCII 0xFE
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn test_beeper_intermittent() {
        assert_eq!(
            Beeper::new_intermittent(Duration::from_millis(200), Duration::from_secs(24)),
            Beeper::Intermittent {
                on_time: 0x02,
                off_time: 0xF0
            },
        )
    }

    #[test]
    fn test_update_flag() {
        let mut flag: UpdateFlag<0xFF> = UpdateFlag(false);
        assert_eq!(flag.get_toggle(), 0x0);
        assert_eq!(flag.get_toggle(), 0xFF);
        assert_eq!(flag.get_toggle(), 0x0);
    }
}
