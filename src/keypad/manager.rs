use std::{error::Error, sync::Arc, time::Duration};

use crate::serial::devices::keypad::SerialKeypad;

const SYSTEM_OWNER: &'static str = "TIGER SECURITY";

enum DisplayMode {
    Idle,
}

pub struct KeypadManager {
    keypad: Arc<SerialKeypad>,

    state: DisplayMode,
}

impl KeypadManager {
    pub fn new(keypad: Arc<SerialKeypad>) -> KeypadManager {
        KeypadManager {
            keypad,
            state: DisplayMode::Idle,
        }
    }

    pub async fn run(&mut self) -> Result<(), Box<dyn Error + Sync + Send>> {
        {
            let keypad = self.keypad.clone();
            tokio::spawn(async move { keypad.as_ref().update_worker().await });
        }

        loop {
            let next_wakeup = match self.state {
                DisplayMode::Idle => {
                    let mut banner = format!("{:<16}", SYSTEM_OWNER);
                    if self.keypad.is_tamper() {
                        banner.pop();
                        banner.push('T');
                    }

                    self.keypad.mutate_state(|state| {
                        state.screen.lines = [
                            banner,
                            chrono::Local::now()
                                .format("%a %_d %b %H:%M")
                                .to_string()
                                .to_uppercase(),
                        ]
                    });

                    Duration::from_millis(250)
                }
            };

            tokio::time::sleep(next_wakeup).await;
        }
    }
}
