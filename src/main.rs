use std::{
    collections::HashMap,
    env,
    error::Error,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use ::galaxy::serial::{galaxy::Bus, manager::SerialManager, SerialDevice};
use galaxy::{keypad::manager::KeypadManager, serial::devices::keypad::SerialKeypad};
use log::debug;
use tokio::runtime;
use tokio_serial::{self, SerialStream};

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Trace)
        .init();

    let args: Vec<String> = env::args().collect();

    if args.len() != 2 {
        eprintln!("Usage: {} SERIAL_ADAPTER", args[0]);
        return Err("Missing mandatory serial path argument".into());
    }

    let rt = runtime::Builder::new_multi_thread()
        .enable_io()
        .enable_time()
        .thread_name_fn(|| {
            static ATOMIC_ID: AtomicUsize = AtomicUsize::new(0);
            let id = ATOMIC_ID.fetch_add(1, Ordering::SeqCst);
            format!("galaxyd-worker-{}", id)
        })
        .build()
        .expect("unable to build tokio runtime");

    let keypad = Arc::new(SerialKeypad::new());

    let serial_manager = rt.spawn(run_serial_manager(
        args[1].clone(),
        HashMap::from([(0x10u8, keypad.clone() as Arc<dyn SerialDevice>)]),
    ));
    let keypad_worker = rt.spawn(async move { KeypadManager::new(keypad.clone()).run().await });

    rt.block_on(serial_manager)??;
    rt.block_on(keypad_worker)??;

    Ok(())
}

async fn run_serial_manager(
    serial_device: String,
    devices: HashMap<u8, Arc<dyn SerialDevice>>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut serial_stream = SerialStream::open(
        &tokio_serial::new(serial_device, 9600)
            .data_bits(tokio_serial::DataBits::Eight)
            .stop_bits(tokio_serial::StopBits::One)
            .parity(tokio_serial::Parity::None)
            .flow_control(tokio_serial::FlowControl::None)
            .timeout(Duration::from_millis(100)),
    )?;
    if !serial_stream.exclusive() {
        serial_stream
            .set_exclusive(true)
            .map_err(|e| format!("Unable to exclusively acquire serial port: {}", e))?;
    }

    let mut serial_manager = SerialManager::new(Bus::new(serial_stream));
    for (address, device) in devices {
        serial_manager.register_device(address, device);
    }

    debug!("Starting serial manager");

    serial_manager.run().await;

    Ok(())
}
