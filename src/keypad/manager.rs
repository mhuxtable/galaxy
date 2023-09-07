use std::{
    error::Error,
    sync::{Arc, Mutex},
    time::Duration,
};

use crate::serial::devices::keypad::{Backlight, Event, EventType, SerialKeypad};

const SYSTEM_OWNER: &'static str = "TIGER SECURITY";

#[derive(PartialEq)]
enum DisplayMode {
    Idle,
    CodeEntry,
    Menu,
}

pub struct KeypadManager {
    keypad: Arc<SerialKeypad>,

    state: Arc<Mutex<DisplayMode>>,
    accumulator: Arc<Mutex<Option<String>>>,
}

impl KeypadManager {
    pub fn new(keypad: Arc<SerialKeypad>) -> KeypadManager {
        KeypadManager {
            keypad,
            state: Arc::new(Mutex::new(DisplayMode::Idle)),
            accumulator: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn run(&mut self) -> Result<(), Box<dyn Error + Sync + Send>> {
        let mut event_ch = self.keypad.subscribe_events();

        let mut time_updater = {
            use std::time::SystemTime;
            use tokio::time::{interval_at, Instant};

            let now = SystemTime::now();
            let next_minute = (now
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs()
                / 60
                + 1)
                * 60;
            let start_instant = Instant::now()
                + Duration::from_secs(
                    next_minute
                        - now
                            .duration_since(SystemTime::UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                );

            interval_at(start_instant, Duration::from_secs(60))
        };

        loop {
            tokio::select! {
                _ = time_updater.tick() => {
                    self.update_keypad_state();
                }
                msg = event_ch.recv() => {
                    match msg {
                        Ok(event) => {
                            self.process_event(event);
                            self.update_keypad_state();
                        }
                        Err(_) => {
                            // TODO fix me!
                            panic!("KeypadManager encountered error receiving event")
                        }
                    }
                }
            }
        }
    }

    fn update_keypad_state(&mut self) {
        let mut state = self.state.lock().unwrap();

        match *state {
            DisplayMode::Idle => {
                let mut banner = format!("{:<16}", SYSTEM_OWNER);
                if self.keypad.is_tamper() {
                    banner.pop();
                    banner.push('T');
                }

                self.keypad.mutate_state(|state| {
                    state.backlight = Backlight::Off;
                    state.blink = false;
                    state.screen.lines = [
                        banner,
                        chrono::Local::now()
                            .format("%a %_d %b %H:%M")
                            .to_string()
                            .to_uppercase(),
                    ]
                });
            }
            DisplayMode::CodeEntry => {
                let acc = self.accumulator.lock().unwrap();

                if acc.as_ref().is_some_and(|data| data == "1234E") {
                    *state = DisplayMode::Menu;
                } else {
                    let line1 = if acc.is_some() {
                        acc.clone().unwrap()
                    } else {
                        "".to_string()
                    };

                    self.keypad.mutate_state(|state| {
                        state.backlight = Backlight::On;
                        state.blink = false;
                        state.screen.lines = [line1, "".to_string()];
                    });
                }
            }
            DisplayMode::Menu => {
                self.keypad.mutate_state(|state| {
                    state.backlight = Backlight::On;
                    state.blink = true;
                    state.screen.lines =
                        ["10 = SETTING".to_string(), "[ent] to select".to_string()];
                });
            }
        }
    }

    fn process_event(&mut self, event: Event) {
        let mut state = self.state.lock().unwrap();
        let mut acc = self.accumulator.lock().unwrap();

        match event.0 {
            EventType::KeyPress(key) => {
                if *state == DisplayMode::Idle && key != 'X' {
                    let mut s = String::with_capacity(16);
                    s.push(key);

                    *state = DisplayMode::CodeEntry;
                    *acc = Some(s);
                } else if *state == DisplayMode::CodeEntry {
                    match key {
                        'X' => {
                            *state = DisplayMode::Idle;
                            *acc = None;
                        }
                        x => acc.as_mut().unwrap().push(x),
                    }
                } else if *state == DisplayMode::Menu && key == 'X' {
                    *state = DisplayMode::Idle;
                }
            }
        }
    }
}
