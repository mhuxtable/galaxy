use std::{
    error::Error,
    sync::{Arc, Mutex},
    time::Duration,
};

use log::debug;
use tokio::time::Interval;

use crate::serial::devices::keypad::{Backlight, Event, EventType, SerialKeypad};

const SYSTEM_OWNER: &'static str = "TIGER SECURITY";

#[derive(Clone, Copy, PartialEq)]
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
        use backlight_responder::BacklightResponder;

        let mut event_ch = self.keypad.subscribe_events();
        let mut time_updater_interval = interval_at_next_minute();

        // TODO stop the responder when it's time to shut down
        let (_backlight_responder_token, backlight_state_tx) = {
            let (mut backlight_responder, state_tx) = {
                let backlight_keypad = self.keypad.clone();

                BacklightResponder::new(Box::new(move |backlight_state| {
                    backlight_keypad.mutate_state(|state| state.backlight = backlight_state)
                }))
            };

            (
                tokio::spawn(async move { backlight_responder.run().await }),
                state_tx,
            )
        };

        // Initial start tick
        self.update_keypad_state();

        loop {
            tokio::select! {
                _ = time_updater_interval.tick() => {
                    self.update_keypad_state();
                }
                msg = event_ch.recv() => {
                    debug!("Received keypad event: {:?}", msg);

                    match msg {
                        Ok(event) => {
                            let new_state = self.process_event(event);
                            // TODO handle the Result error
                            backlight_state_tx.send(new_state)?;
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
        let state = self.state.lock().unwrap();
        let banner = format!("{:<16}", SYSTEM_OWNER);

        match *state {
            DisplayMode::Idle => {
                self.keypad.mutate_state(|state| {
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
                self.keypad.mutate_state(|state| {
                    state.backlight = Backlight::On;
                    state.screen.lines = ["".to_string(), "".to_string()];
                });

                let acc = self.accumulator.lock().unwrap();

                let line1 = if acc.is_some() {
                    acc.clone().unwrap()
                } else {
                    "".to_string()
                };

                self.keypad.mutate_state(|state| {
                    state.blink = false;
                    state.screen.lines = [line1, "".to_string()];
                });
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

    fn process_event(&mut self, event: Event) -> DisplayMode {
        let mut state = self.state.lock().unwrap();
        let mut acc = self.accumulator.lock().unwrap();

        match event.0 {
            EventType::KeyPress(key) => {
                if key == 'X' {
                    *state = DisplayMode::Idle;
                } else if *state == DisplayMode::Idle && key != 'X' {
                    let mut s = String::with_capacity(16);
                    s.push(key);

                    *state = DisplayMode::CodeEntry;
                    *acc = Some(s);
                } else if *state == DisplayMode::CodeEntry {
                    acc.as_mut().unwrap().push(key);

                    if acc.as_ref().unwrap() == "1234E" {
                        *state = DisplayMode::Menu;
                    }
                }
            }
        }

        *state
    }
}

mod backlight_responder {
    use log::debug;
    use std::{error::Error, sync::Arc, time::Duration};
    use tokio::{
        sync::{mpsc, oneshot},
        time,
    };

    use crate::serial::devices::keypad::Backlight;

    use super::DisplayMode;

    pub(super) struct BacklightResponder {
        rx: mpsc::UnboundedReceiver<DisplayMode>,
        last_state: Option<DisplayMode>,
        backlight_control: Arc<Box<dyn Fn(Backlight) + Send + Sync>>,
        timeout: Duration,
    }

    impl BacklightResponder {
        pub fn new(
            controller: Box<dyn Fn(Backlight) + Send + Sync>,
        ) -> (BacklightResponder, mpsc::UnboundedSender<DisplayMode>) {
            let (tx, rx) = mpsc::unbounded_channel();

            (
                BacklightResponder {
                    rx,
                    last_state: None,
                    backlight_control: Arc::new(controller),
                    timeout: Duration::from_secs(5),
                },
                tx,
            )
        }

        pub async fn run(&mut self) -> Result<(), Box<dyn Error + Sync + Send>> {
            let mut cancel_token: Option<oneshot::Sender<()>> = None;

            loop {
                match self.rx.recv().await {
                    Some(display_mode) => {
                        if self
                            .last_state
                            .is_some_and(|last_state| last_state == display_mode)
                        {
                            continue;
                        }

                        self.last_state = Some(display_mode);

                        if let Some(cancel_token) = cancel_token.take() {
                            debug!(
                                "BacklightResponder: cancelling last task as entering new state"
                            );
                            let _ = cancel_token.send(());
                        }

                        match display_mode {
                            DisplayMode::Idle => {
                                // Start a timer to switch off the backlight after a period of time.
                                let (cancel_tx, cancel_rx) = oneshot::channel();
                                cancel_token = Some(cancel_tx);

                                let backlight_control = self.backlight_control.clone();
                                debug!("BacklightResponder: spawning task to toggle backlight state after quiescent period");

                                let timeout_duration = self.timeout.clone();

                                tokio::spawn(async move {
                                    tokio::select! {
                                        _ = time::sleep(timeout_duration) => {
                                            debug!("BacklightResponder: toggling backlight as timer fired");
                                            backlight_control(Backlight::Off);
                                        }
                                        _ = cancel_rx => {},
                                    }
                                });
                            }
                            // In any state change away from Idle, the cancel_token was already
                            // cancelled earlier.
                            _ => {}
                        }
                    }
                    None => break,
                }
            }

            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use std::sync::{Arc, Mutex};
        use tokio::{sync::Notify, time};

        use super::*;

        fn instantiate_backlight_responder() -> (
            Arc<Mutex<Option<Backlight>>>,
            Arc<Notify>,
            BacklightResponder,
            mpsc::UnboundedSender<DisplayMode>,
        ) {
            let tx_backlight_state = Arc::new(Mutex::new(None));
            let notify = Arc::new(Notify::new());
            let (backlight_responder, tx) = {
                let state = tx_backlight_state.clone();
                let notify = notify.clone();

                BacklightResponder::new(Box::new(move |new_state| {
                    *state.lock().unwrap() = Some(new_state);
                    notify.notify_waiters();
                }))
            };

            (tx_backlight_state, notify, backlight_responder, tx)
        }

        #[tokio::test]
        async fn test_backlight_goes_off_after_idle() {
            time::pause();

            let (state, notify, mut responder, tx) = instantiate_backlight_responder();

            tokio::spawn(async move {
                responder.run().await.unwrap();
            });

            tx.send(DisplayMode::Idle).unwrap();

            let tx_backlight_state_clone = state.clone();

            // Resume time to trigger the timer
            time::advance(Duration::from_secs(5)).await;
            notify.notified().await;

            assert_eq!(
                *tx_backlight_state_clone.lock().unwrap(),
                Some(Backlight::Off)
            );
        }

        #[tokio::test]
        async fn test_backlight_stays_on_if_not_idle() {
            time::pause();

            let (state, notify, mut responder, tx) = instantiate_backlight_responder();
            *state.lock().unwrap() = Some(Backlight::On);

            let handle = tokio::spawn(async move {
                responder.run().await.unwrap();
            });

            tx.send(DisplayMode::Idle).unwrap();
            tx.send(DisplayMode::Menu).unwrap();

            let tx_backlight_state_clone = state.clone();

            // Resume time to trigger the timer
            time::advance(Duration::from_secs(6)).await;
            drop(tx);

            assert!(handle.await.is_ok());
            tokio::time::sleep(Duration::from_secs(1)).await;

            assert_eq!(
                *tx_backlight_state_clone.lock().unwrap(),
                Some(Backlight::On)
            );
        }

        #[tokio::test]
        async fn test_receiver_closes_loop_breaks() {
            let (mut backlight_responder, tx) = BacklightResponder::new(Box::new(|_| {}));

            drop(tx); // simulate receiver closing

            assert_eq!(backlight_responder.run().await.is_ok(), true);
        }
    }
}

fn interval_at_next_minute() -> Interval {
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
}
