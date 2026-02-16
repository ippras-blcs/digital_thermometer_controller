

==== src\deadline.rs ====

use anyhow::Result;
use esp_idf_svc::sntp::{EspSntp, SyncStatus};
use log::debug;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::{spawn, time::sleep};

const SLEEP: Duration = Duration::from_secs(1);
const TIMESTAMP: Duration = Duration::from_secs(1767214800);

pub(super) fn start() {
    spawn(run());
}

pub(super) async fn run() -> Result<()> {
    let sntp = EspSntp::new_default()?;
    while SyncStatus::Completed != sntp.get_sync_status() {
        debug!("SNTP is not completed, wait {}", SLEEP.as_secs());
        sleep(SLEEP).await;
    }
    if SystemTime::now().duration_since(UNIX_EPOCH)? > TIMESTAMP {
        panic!("TIMESTAMP: {TIMESTAMP:?}");
    }
    Ok(())
}


==== src\main.rs ====

#![feature(impl_trait_in_assoc_type)]

use anyhow::Result;
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    hal::{prelude::Peripherals, reset::restart},
    io::vfs::MountedEventfs,
    log::EspLogger,
    nvs::EspDefaultNvsPartition,
    sys::link_patches,
    timer::EspTaskTimerService,
    wifi::WifiEvent,
};
use log::{error, info, warn};
use tokio::runtime::Builder;
use wifi::connect;

const _MAC_ADDRESS: &str = "7c:df:a1:a3:5a:f8";

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
    // Start deadline checker
    deadline::start();
    // Start temperature reader
    let temperature_sender = temperature::start(peripherals.pins.gpio2, peripherals.rmt.channel0)?;
    // Run modbus server
    modbus::run(temperature_sender.clone()).await?;
    Ok(())
}

mod deadline;
mod modbus;
mod temperature;
mod wifi;


==== src\modbus.rs ====

use crate::temperature::Request as TemperatureRequest;
use anyhow::Result;
use log::{error, info};
use std::{net::SocketAddr, sync::LazyLock};
use tokio::{
    net::TcpListener,
    sync::{mpsc::Sender, oneshot},
};
use tokio_modbus::{
    prelude::*,
    server::{
        Service,
        tcp::{Server, accept_tcp_connection},
    },
};

const INPUT_REGISTER_SIZE: usize = 6;

static SOCKET_ADDR: LazyLock<SocketAddr> = LazyLock::new(|| "0.0.0.0:5502".parse().unwrap());

pub(super) async fn run(temperature_sender: Sender<TemperatureRequest>) -> Result<()> {
    let server = Server::new(TcpListener::bind(*SOCKET_ADDR).await?);
    let new_service = |_socket_addr| Ok(Some(ExampleService::new(temperature_sender.clone())));
    let on_connected = |stream, socket_addr| async move {
        accept_tcp_connection(stream, socket_addr, new_service)
    };
    let on_process_error = |error| error!("{error}");
    server.serve(&on_connected, on_process_error).await?;
    Ok(())
}

struct ExampleService {
    temperature_sender: Sender<TemperatureRequest>,
}

impl ExampleService {
    fn new(temperature_sender: Sender<TemperatureRequest>) -> Self {
        Self { temperature_sender }
    }
}

impl Service for ExampleService {
    type Request = Request<'static>;
    type Response = Response;
    type Exception = ExceptionCode;
    type Future = impl Future<Output = Result<Self::Response, Self::Exception>>;

    fn call(&self, request: Self::Request) -> Self::Future {
        info!("Modbus request: {request:?}");
        let temperature_sender = self.temperature_sender.clone();
        async move {
            match request {
                Request::ReadInputRegisters(address, count) => {
                    let address = address as usize;
                    let count = count as usize;
                    if address % INPUT_REGISTER_SIZE != 0 || count % INPUT_REGISTER_SIZE != 0 {
                        error!("IllegalAddress {{ address: {address}, count: {count} }}");
                        return Err(ExceptionCode::IllegalDataAddress);
                    }
                    let start = address / INPUT_REGISTER_SIZE;
                    let end = start + count / INPUT_REGISTER_SIZE;
                    let (sender, receiver) = oneshot::channel();
                    if let Err(error) = temperature_sender.send((start..end, sender)).await {
                        error!("{error:?}");
                        return Err(ExceptionCode::ServerDeviceFailure);
                    };
                    let input_registers: Vec<_> = match receiver.await {
                        Ok(Ok(temperatures)) => temperatures
                            .into_iter()
                            .flat_map(|(address, temperature)| {
                                let address = address.to_be_bytes();
                                let temperature = temperature.to_be_bytes();
                                [
                                    u16::from_be_bytes([address[0], address[1]]),
                                    u16::from_be_bytes([address[2], address[3]]),
                                    u16::from_be_bytes([address[4], address[5]]),
                                    u16::from_be_bytes([address[6], address[7]]),
                                    u16::from_be_bytes([temperature[0], temperature[1]]),
                                    u16::from_be_bytes([temperature[2], temperature[3]]),
                                ]
                            })
                            .collect(),
                        Ok(Err(error)) => {
                            error!("{error:?}");
                            return Err(error.into());
                        }
                        Err(error) => {
                            error!("{error:?}");
                            return Err(ExceptionCode::ServerDeviceFailure);
                        }
                    };
                    Ok(Response::ReadInputRegisters(
                        input_registers[address..count].to_vec(),
                    ))
                }
                _ => Err(ExceptionCode::IllegalFunction),
            }
        }
    }
}


==== src\temperature.rs ====

use esp_idf_svc::hal::{gpio::IOPin, onewire::OWAddress, peripheral::Peripheral, rmt::RmtChannel};
use log::{info, trace};
use std::{collections::BTreeMap, ops::Range};
use thermometer::{
    Ds18b20Driver,
    scratchpad::{ConfigurationRegister, Resolution, Scratchpad},
};
use thiserror::Error;
use tokio::{
    spawn,
    sync::{
        mpsc::{self, Sender},
        oneshot::Sender as OneshotSender,
    },
};
use tokio_modbus::prelude::ExceptionCode;

pub(crate) type Request = (
    Range<usize>,
    OneshotSender<Result<BTreeMap<u64, f32>, Error>>,
);

pub(super) fn start(
    pin: impl Peripheral<P = impl IOPin> + 'static,
    channel: impl Peripheral<P = impl RmtChannel> + 'static,
) -> Result<Sender<Request>> {
    info!("Initialize temperature reader");
    let mut driver = Ds18b20Driver::new(pin, channel)?;
    info!("Temperature driver initialized");
    let mut addresses = driver.search()?.collect::<Result<Vec<OWAddress>, _>>()?;
    addresses.sort_by_key(OWAddress::address);
    for address in &addresses {
        let scratchpad = driver
            .initialization()?
            .match_rom(address)?
            .read_scratchpad()?;
        info!("{address:x?}: {scratchpad:?}");
    }
    for address in &addresses {
        driver
            .initialization()?
            .match_rom(address)?
            .write_scratchpad(&Scratchpad {
                alarm_high_trigger_register: 30,
                alarm_low_trigger_register: 10,
                configuration_register: ConfigurationRegister {
                    resolution: Resolution::Twelve,
                },
                ..Default::default()
            })?;
    }
    for address in &addresses {
        let scratchpad = driver
            .initialization()?
            .match_rom(address)?
            .read_scratchpad()?;
        info!("{address:x?}: {scratchpad:?}");
    }
    let (sender, mut receiver) = mpsc::channel::<Request>(9);
    info!("Spawn temperature reader");
    spawn(async move {
        while let Some((indices, sender)) = receiver.recv().await {
            trace!("Read temperatures {indices:?}");
            let _ = sender.send((|| {
                trace!("Send temperatures {indices:?}");
                if indices.end > addresses.len() {
                    return Err(Error::InvalidIndex {
                        received: indices,
                        expected: 0..addresses.len(),
                    });
                }
                let mut temperatures = BTreeMap::new();
                driver.initialization()?.skip_rom()?.convert_temperature()?;
                for index in indices {
                    let address = &addresses[index];
                    let temperature = driver
                        .initialization()?
                        .match_rom(address)?
                        .read_scratchpad()?
                        .temperature;
                    trace!("{address:x?}: {temperature}");
                    temperatures.insert(address.address(), temperature);
                }
                Ok(temperatures)
            })());
        }
    });
    Ok(sender)
}

/// Result
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Error
#[derive(Error, Debug)]
pub enum Error {
    #[error("Invalid index {{ received: {received:?}, expected: {expected:?} }}")]
    InvalidIndex {
        received: Range<usize>,
        expected: Range<usize>,
    },
    #[error(transparent)]
    Internal(#[from] thermometer::Error),
}

impl From<Error> for ExceptionCode {
    fn from(value: Error) -> Self {
        match value {
            Error::InvalidIndex { .. } => ExceptionCode::IllegalDataAddress,
            Error::Internal(_) => ExceptionCode::ServerDeviceFailure,
        }
    }
}


==== src\wifi.rs ====

use esp_idf_svc::{
    eventloop::{EspEventLoop, System},
    hal::modem::Modem,
    nvs::EspDefaultNvsPartition,
    sys::EspError,
    timer::{EspTimerService, Task},
    wifi::{AsyncWifi, ClientConfiguration, Configuration, EspWifi},
};
use log::info;

const SSID: &str = env!("SSID");
const PASSWORD: &str = env!("PASSWORD");

pub(super) async fn connect(
    modem: Modem,
    event_loop: EspEventLoop<System>,
    timer: EspTimerService<Task>,
    nvs: Option<EspDefaultNvsPartition>,
) -> Result<EspWifi<'static>, EspError> {
    let mut esp_wifi = EspWifi::new(modem, event_loop.clone(), nvs.clone())?;
    let mut wifi = AsyncWifi::wrap(&mut esp_wifi, event_loop.clone(), timer)?;
    wifi.set_configuration(&Configuration::Client(ClientConfiguration {
        ssid: SSID.try_into().unwrap(),
        password: PASSWORD.try_into().unwrap(),
        ..Default::default()
    }))?;

    wifi.start().await?;
    info!("Wifi started");
    wifi.connect().await?;
    info!("Wifi connected");
    wifi.wait_netif_up().await?;
    info!("Wifi netif up");
    Ok(esp_wifi)
}
