use log::{debug, error, info, trace};
use std::sync::{Mutex, RwLock};
use std::time::Duration;
use thiserror::Error;

use crate::serial::{DeliveryError, SerialDevice, SerialMessage};

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
    pub backlight: Backlight, // LCD backlight
    pub beeper: Beeper,       // keypad sounder
    // Updated as part of the screen command, but not part of display state; sent as flags.
    pub blink: bool,           // LED blink state (true = flash, false = steady).
    pub key_clicks: KeyClicks, // chirp when pressing keys
    pub screen: display::KeypadDisplayState, // contents of the display
}

impl Default for State {
    fn default() -> Self {
        State {
            backlight: Backlight::Off,
            beeper: Beeper::Off,
            key_clicks: KeyClicks::Off,
            screen: display::KeypadDisplayState::default(),
            blink: false,
        }
    }
}

#[derive(Clone, Debug)]
pub enum EventType {
    KeyPress(char),
}

#[derive(Clone, Debug)]
pub struct Event(pub EventType);

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

    event_ch: Mutex<tokio::sync::broadcast::Sender<Event>>,
}

impl Default for SerialKeypad {
    fn default() -> Self {
        Self {
            state: RwLock::new(State::default()),
            last_state: RwLock::new(None),
            tamper: Mutex::new(false),
            updates: Mutex::new(KeypadUpdates::default()),

            event_ch: Mutex::new(tokio::sync::broadcast::Sender::new(10)),
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

    pub fn subscribe_events(&self) -> tokio::sync::broadcast::Receiver<Event> {
        self.event_ch.lock().unwrap().subscribe()
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
        } else if updates.send_screen
            || current_state.screen != last_state.screen
            // Blink is not captured on screen because it complicates the diff algorithm.
            || current_state.blink != last_state.blink
        {
            let screen = &current_state.screen;

            // TODO investigate what 0x1F command does in screen updates

            let display_flags = 0x01
                | updates.screen_update_flag.get_toggle()
                | if updates.send_key_ack {
                    updates.send_key_ack = false;
                    0x10 | updates.key_update_flag.get_toggle()
                } else {
                    0
                }
                | if current_state.blink { 0x08 } else { 0 };

            let mut data = vec![display_flags];
            data.extend(if updates.send_screen {
                // Send the full update when sending is forced, to avoid any sync issues between
                // our state and the screen state.
                let (data, _) = screen.full_update();

                data
            } else {
                screen.strategic_update(&last_state.screen)
            });

            updates.send_screen = false;
            last_state.screen = screen.clone();

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
    fn next_message(&self) -> (u8, Option<Vec<u8>>) {
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

        (command.into(), data)
    }

    fn receive_update(&self, msg: Result<SerialMessage, DeliveryError>) {
        let ev_ch = self.event_ch.lock().unwrap().clone();

        trace!("got update: {:?}", msg);

        let mut last_state = self
            .last_state
            .write()
            .expect("unable to lock last_state for writing");

        let mut tamper = self
            .tamper
            .lock()
            .expect("unable to lock tamper for writing");

        let mut updates = self
            .updates
            .lock()
            .expect("unable to lock update state for reading");

        match msg {
            Ok(reply) => {
                match ReplyCommand::try_from(reply.command) {
                    Ok(ReplyCommand::Initialised) => {
                        if last_state.is_some() {
                            // TODO figure out how to handle an unexpected init
                            error!(
                                "Received initialise response for an already initialised keypad"
                            );
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
                            // TODO handle this error
                            return;
                        }

                        let data = reply.additional_data.unwrap()[0];

                        if data == 0x7F {
                            *tamper = true;
                        } else {
                            *tamper = data & 0x40 == 0x40;

                            // Only handle a key press event if a key acknowledgement is not
                            // pending. A pending ACK means a reported key press will be a
                            // duplicate, as no acknowledgement has yet been sent to the panel.
                            //
                            // This situation arises because the sequence of events sent back to
                            // the keypad does not always prioritise sending key acks over other
                            // user interface indications; key presses stack, which can starve the
                            // transmission of other updates that provide reassurance of activity
                            // to the user, e.g. backlight changes, so key press ACKs are treated
                            // with lower priority.
                            if !updates.send_key_ack {
                                let key_press = key_to_char(data & 0xF);
                                info!("Received key press of {} from keypad", key_press);

                                updates.send_key_ack = true;

                                // TODO deal with this Result
                                let _ = ev_ch.send(Event(EventType::KeyPress(key_press)));
                            }
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

mod display {
    use log::{debug, trace};

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

    fn pad_string_iterator<'a>(length: usize, s: &'a str) -> impl Iterator<Item = char> + 'a {
        s.chars().chain(std::iter::repeat(' ')).take(length)
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct KeypadDisplayState {
        pub lines: [String; 2],
        // If None, we don't care where the cursor currently is, and we allow it to float. This
        // reduces the size of update messages.
        pub cursor_position: Option<u8>,
        pub cursor_style: CursorStyle,
    }

    impl KeypadDisplayState {
        pub(super) fn strategic_update(&self, from: &KeypadDisplayState) -> Vec<u8> {
            // Display updates can be performed in full or in part. The cost of each is as follows:
            //
            // full update: reset the display, seek the cursor and output characters to relevant
            //              positions. We don't need to write whitespace, but sometimes it will be
            //              cheaper to do so than seeking past it
            //
            // partial update: seek cursor to relevant position (two bytes), write out characters
            //                 (byte per character), repeat for further changed blocks. Also reset
            //                 cursor position and style if required.
            //
            // It is preferred to do partial updates where efficient, but in some cases a full
            // update will be more prudent on bus time.
            //
            // Update score is computed as a heuristic to determine the mechanism used for screen
            // update. This is determined as 2*blocks_changed+chars_diff. blocks_changed is
            // computed by splitting the string into blocks of non-contiguous modified characters,
            // starting a new block when matching characters are identified.
            //
            // A full update is performed if the update score is greater than the cost of a full
            // update of the display, computed as 1 (display reset cost) + 2 (start of line seek
            // cost, 1 per line) + sum(unpadded_line_length).
            //
            // The cost of updating cursor positions is omitted as it must be performed on either
            // update style.
            //
            // The cost of updating the cursor style is only included if this has changed.
            //
            // This heuristic will overpredict the cost of partial updates relative to the most
            // efficient algorithm. This arises because the cost of seeking the cursor to the start
            // of a block is 2 bytes, meaning update blocks separated by at most one unchanged
            // character can be more efficiently updated by fusing the blocks and writing out the
            // unchanged character rather than seeking the cursor:
            //
            // Before:  AABBCC
            // After:   ABBCCC
            // Changes:  ^ ^
            //
            // Update score          = 2 * 2 blocks + 2 chars = 6
            // Most efficient update = seek 0x1 (cost 2) + print BBC (cost 3) = 5
            //
            // This situation is ignored for heuristic purposes but is accounted for in the partial
            // update algorithm, which does fuse blocks together where it yields an efficiency
            // saving in the generated update for the bus.
            let (update_score, full_score) = (
                self.update_score(from),
                3 + self.lines.iter().map(|line| line.len()).sum::<usize>(),
            );
            trace!(
                "keypad display strategic update: update score: {}  full score: {}",
                update_score,
                full_score
            );

            // Keypads have a maximum message length they will process which is determined by bus
            // timing and the period it will consume data from the bus. This is about 46 symbols;
            // shorten to 40 to allow for the envelope data, plus a small margin.
            const MAX_PARTIAL_UPDATE_SCORE: usize = 40;

            let (mut update, cursor_position) =
                if update_score < full_score && update_score < MAX_PARTIAL_UPDATE_SCORE {
                    self.partial_update(from)
                } else {
                    self.full_update()
                };

            if let Some(offset) = self.cursor_position {
                if cursor_position.map_or(true, |cur_pos| cur_pos as u8 != offset) {
                    update.extend([ScreenOpCodes::CURSOR_SEEK_BYTE, offset]);
                }
            }

            update
        }

        pub(super) fn full_update(&self) -> (Vec<u8>, Option<usize>) {
            let mut data = vec![ScreenOpCodes::DISPLAY_RESET, ScreenOpCodes::CURSOR_HIDDEN];

            // Full update does not require line padding of output lines to display width with
            // whitespace as the display was reset to blank.

            let cursor_position = {
                let mut cursor_position = None;

                for (i, (&op, line)) in [
                    ScreenOpCodes::CURSOR_FIRST_LINE,
                    ScreenOpCodes::CURSOR_SECOND_LINE,
                ]
                .iter()
                .zip(self.lines.iter())
                .enumerate()
                {
                    if line.len() > 0 {
                        data.push(op);
                        data.extend(line.chars().map(|x| x as u8));

                        cursor_position = Some(i * 0x40 + line.len() + 1);
                    }
                }

                cursor_position
            };

            if self.cursor_style != CursorStyle::None {
                data.push(ScreenOpCodes::cursor_style_op_code(self.cursor_style));
            }

            (data, cursor_position)
        }

        fn partial_update(&self, from: &KeypadDisplayState) -> (Vec<u8>, Option<usize>) {
            let mut data = vec![];

            let cursor_final_position = {
                let from = from.lines.iter().map(|line| pad_string_iterator(16, &line));
                let to = self.lines.iter().map(|line| pad_string_iterator(16, &line));

                let mut cursor_position = None;

                const START_OF_LINE_SEEK_OP_CODES: [u8; 2] = [
                    ScreenOpCodes::CURSOR_FIRST_LINE,
                    ScreenOpCodes::CURSOR_SECOND_LINE,
                ];

                for (i, (from, to)) in from.zip(to).enumerate() {
                    // skipped_char is used to 'fuse' changed blocks separated by at most one
                    // unchanged character. It is more efficient to emit the unchanged character as
                    // a data byte in order to advance the cursor to process a subsequent changed
                    // character than it is to seek the cursor (1 byte vs. 2 bytes).
                    let mut skipped_char = None;

                    for (j, (a, b)) in from.zip(to).enumerate() {
                        let offset = i * 0x40 + j;

                        if a != b {
                            // The difference between the current cursor position and the location
                            // required to update the current character.
                            let cursor_diff =
                                cursor_position.map_or(usize::MAX, |cur_pos| offset - cur_pos);

                            // if cursor_diff is 0, it is already in the correct place, so no
                            // action is required.
                            if cursor_diff == 1 && skipped_char.is_some() {
                                data.push(skipped_char.unwrap() as u8);
                            } else if cursor_diff >= 2 {
                                if j == 0 {
                                    data.push(START_OF_LINE_SEEK_OP_CODES[i]);
                                } else {
                                    data.extend([ScreenOpCodes::CURSOR_SEEK_BYTE, offset as u8]);
                                };
                            };

                            skipped_char = None;

                            data.push(b as u8);
                            cursor_position = Some(offset + 1);
                        } else {
                            skipped_char = Some(b);
                        }
                    }
                }

                cursor_position
            };

            if from.cursor_style != self.cursor_style {
                data.push(ScreenOpCodes::cursor_style_op_code(self.cursor_style))
            }

            (data, cursor_final_position)
        }

        fn update_score(&self, from: &KeypadDisplayState) -> usize {
            let lines_score = {
                let from_iter = pad_string_iterator(16, from.lines[0].as_str())
                    .chain(pad_string_iterator(16, from.lines[1].as_str()));
                let to_iter = pad_string_iterator(16, self.lines[0].as_str())
                    .chain(pad_string_iterator(16, self.lines[1].as_str()));

                let mut discrete_blocks = 0;
                let mut chars_diff = 0;
                let mut in_block = false;

                for (a, b) in from_iter.zip(to_iter) {
                    if a != b {
                        chars_diff += 1;
                        if !in_block {
                            discrete_blocks += 1;
                            in_block = true;
                        }
                    } else {
                        in_block = false;
                    }
                }

                2 * discrete_blocks + chars_diff
            };

            let cursor_score = (from.cursor_style != self.cursor_style)
                .then_some(1)
                .unwrap_or(0);

            lines_score + cursor_score
        }
    }

    impl Default for KeypadDisplayState {
        fn default() -> Self {
            KeypadDisplayState {
                lines: [
                    String::from("    ********    "),
                    String::from("Panel booting up"),
                ],
                cursor_position: None,
                cursor_style: CursorStyle::None,
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

        pub fn cursor_style_op_code(cursor_style: CursorStyle) -> u8 {
            match cursor_style {
                CursorStyle::None => ScreenOpCodes::CURSOR_HIDDEN,
                CursorStyle::Block => ScreenOpCodes::CURSOR_BLOCK_STYLE,
                CursorStyle::Underline => ScreenOpCodes::CURSOR_UNDERLINE_STYLE,
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        /// Test screen doing a full update, from a full screen to a very empty screen.
        fn screen_simulate_full_update() {
            let before = KeypadDisplayState {
                lines: ["VERY LONG LINE".to_string(), "SOME MORE TEXT".to_string()],
                cursor_style: CursorStyle::Block,
                cursor_position: Some(0x45),
            };
            let after = KeypadDisplayState {
                lines: ["A".to_string(), "".to_string()],
                cursor_style: CursorStyle::Block,
                cursor_position: Some(0x45),
            };

            let update = after.strategic_update(&before);

            assert_eq!(update, vec![0x17, 0x07, 0x01, 'A' as u8, 0x06, 0x03, 0x45]);
        }

        #[test]
        /// Test screen doing a partial update, with very disparate blocks of updated text spread
        /// across the screen. The updates are constructed so as to test block fusing (differring
        /// characters separated by a single unchanged character), start of line cursor seek
        /// optimisation (use the special commands to jump to 0x0 or 0x40 for a 1 byte reduction)
        /// and sequential character updates (only seek to start of a block, not charaters within a
        /// block as the cursor is advanced automatically).
        fn screen_simulate_partial_update() {
            let before = KeypadDisplayState {
                lines: [
                    "ABCD1234EFGH5678".to_string(),
                    "0123456789ABCDEF".to_string(),
                ],
                cursor_style: CursorStyle::None,
                cursor_position: None,
            };
            let after = KeypadDisplayState {
                lines: [
                    "ABCCC234EEGH8765".to_string(),
                    "1023456789ABCDDD".to_string(),
                ],
                cursor_style: CursorStyle::None,
                cursor_position: None,
            };

            let update = after.strategic_update(&before);
            let expect = vec![
                0x03, 0x03, 0x43, 0x43, 0x03, 0x09, 0x45, 0x03, 0x0C, 0x38, 0x37, 0x36, 0x35, 0x02,
                0x31, 0x30, 0x03, 0x4E, 0x44, 0x44,
            ];

            assert_eq!(
                update, expect,
                "got: {:02X?} expect: {:02X?}",
                update, expect
            );
        }
    }
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
