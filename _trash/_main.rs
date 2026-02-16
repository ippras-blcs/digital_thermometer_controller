#![feature(impl_trait_in_assoc_type)]

use anyhow::Result;
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    hal::{prelude::Peripherals, reset::restart},
    io::vfs::MountedEventfs,
    log::EspLogger,
    nvs::EspDefaultNvsPartition,
    sntp::EspSntp,
    sys::link_patches,
    timer::EspTaskTimerService,
    wifi::WifiEvent,
};
use log::{error, info, warn};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::runtime::Builder;
use wifi::connect;

const MAC_ADDRESS: &str = "7c:df:a1:a3:5a:f8";
// const TIMESTAMP: Duration = Duration::from_secs(1767214800);
const TIMESTAMP: Duration = Duration::from_secs(1742903600);

fn main() -> Result<()> {
    link_patches();
    EspLogger::initialize_default();
    let _mounted_eventfs = MountedEventfs::mount(5)?;
    info!("System initialized");
    if let Err(error) = Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(run())
    {
        error!("{error:?}");
    } else {
        info!("`main()` finished, restarting");
    }
    restart();
}

async fn run() -> Result<()> {
    let event_loop = EspSystemEventLoop::take()?;
    let timer = EspTaskTimerService::new()?;
    let peripherals = Peripherals::take()?;
    let nvs = EspDefaultNvsPartition::take()?;
    // Initialize the network stack, this must be done before starting the server
    let mut wifi = connect(peripherals.modem, event_loop.clone(), timer, Some(nvs)).await?;
    let _subscription = event_loop.subscribe::<WifiEvent, _>(move |event| {
        info!("Got event: {event:?}");
        if let WifiEvent::StaDisconnected(_) = event {
            if let Err(error) = wifi.connect() {
                warn!("Wifi connect failed: {error}");
            }
        }
    })?;
    // Keep it around or else the SNTP service will stop
    let _sntp = EspSntp::new_default()?;
    info!("SNTP initialized");
    let now = SystemTime::now();
    error!("now: {now:?}");
    error!("TIMESTAMP: {TIMESTAMP:?}");
    if now < UNIX_EPOCH + TIMESTAMP {
        panic!("TIMESTAMP: {TIMESTAMP:?}");
    }
    // Run temperature reader
    let temperature_sender = temperature::run(peripherals.pins.gpio2, peripherals.rmt.channel0)?;
    modbus::run(temperature_sender.clone()).await?;
    // select! {
    //     // Run MQTT server
    //     // _ = mqtt::run(temperature_sender.clone()) => {},
    //     // Run modbus server
    //     _ = modbus::run(temperature_sender.clone()) => {},
    // }
    Ok(())
}

mod modbus;
mod temperature;
mod wifi;
