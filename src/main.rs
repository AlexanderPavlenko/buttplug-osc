use std::{thread, sync::{Arc, Mutex}};
use async_std::stream::StreamExt;
use async_std::task::block_on;
use structopt::StructOpt;
use url::Url;
use nannou_osc as osc;
use nannou_osc::rosc::OscType;
use buttplug::{
    client::{ButtplugClient, ButtplugClientDevice, ButtplugClientEvent,
             device::VibrateCommand},
    connector::{ButtplugRemoteClientConnector, ButtplugWebsocketClientTransport},
    core::messages::serializer::ButtplugClientJSONSerializer,
};
use anyhow::{bail, Result, Error};
use tracing::{debug, info, warn};

const DEVICES_ALL: &str = "all";
const DEVICES_LAST: &str = "last";

#[derive(StructOpt)]
/// Control https://buttplug.io/ devices via OSC
struct CliArgs {
    #[structopt(long, default_value = "ws://127.0.0.1:12345")]
    intiface_connect: Url,

    #[structopt(long, default_value = "udp://0.0.0.0:9000")]
    osc_listen: Url,

    #[structopt(long = "log-level", env = "RUST_LOG", default_value = "debug")]
    rust_log: String,
}

#[async_std::main]
async fn main() -> Result<()> {
    let args = CliArgs::from_args();
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_env_filter(tracing_subscriber::EnvFilter::new(args.rust_log))
        .with_thread_names(true)
        .init();

    let osc_listen_host_port = validate_osc_listen_url(&args.osc_listen);
    let (devices_r, devices_w) = evmap::new();
    thread::Builder::new().name(String::from("OSC Server")).spawn(move || {
        info!("Starting OSC Server ({})", osc_listen_host_port);
        let _ = block_on(osc_listen(&osc_listen_host_port, devices_r));
    })?;

    let devices_m = Arc::new(Mutex::new(devices_w));
    loop {
        let address = String::from(args.intiface_connect.as_str());
        let devices = devices_m.clone();
        let client_thread =
            thread::Builder::new().name(String::from("Intiface Client")).spawn(move || {
                info!("Starting Intiface Client ({})", address);
                let _ = block_on(intiface_connect(&address, &mut *devices.lock().unwrap()));
            })?;
        client_thread.join().expect("unexpected");
    }
}

async fn intiface_connect(address: &str, devices: &mut evmap::WriteHandle<&str, Device>) -> Result<()> {
    // https://buttplug-developer-guide.docs.buttplug.io/writing-buttplug-applications/device-enum.html#device-connection-events-and-storage
    // > The server could already be running and have devices connected to it. In this case, the Client will emit DeviceAdded events on successful connection.
    // > This means you will want to have your event handlers set up BEFORE connecting, in order to catch these messages.

    let client = ButtplugClient::new("buttplug-osc");
    let mut event_stream = client.event_stream();
    let event_loop = async {
        while let Some(event) = event_stream.next().await {
            match event {
                ButtplugClientEvent::DeviceAdded(device) => {
                    let name = Box::leak(
                        normalize_device_name(&device.name).into_boxed_str());
                    devices.update(name, Device { device: device.clone() });
                    devices.update(DEVICES_LAST, Device { device: device.clone() });
                    devices.refresh();
                    info!("[{}] added", name);
                }
                ButtplugClientEvent::DeviceRemoved(device) => {
                    info!("[{}] removed", normalize_device_name(&device.name));
                    // rescanning, maybe a temporary disconnect
                    let _ = client.stop_scanning().await;
                    let _ = client.start_scanning().await;
                }
                ButtplugClientEvent::ServerDisconnect => {
                    bail!("ServerDisconnect");
                }
                _ => {}
            }
        };
        Ok::<(), Error>(())
    };

    let connector = ButtplugRemoteClientConnector::<
        ButtplugWebsocketClientTransport,
        ButtplugClientJSONSerializer,
    >::new(ButtplugWebsocketClientTransport::new_insecure_connector(address));
    client.connect(connector).await?;
    client.start_scanning().await?;
    event_loop.await
}

fn normalize_device_name(name: &str) -> String {
    name.split(|c: char| !c.is_alphanumeric()).collect::<String>()
}

async fn osc_listen(host_port: &str, devices: evmap::ReadHandle<&'static str, Device>) {
    let rx = osc::Receiver::bind_to(host_port).expect("Invalid --osc-listen: couldn't bind socket");
    for packet in rx.iter() {
        let messages = packet.0.into_msgs();
        for message in messages {
            if let Some(broadcast) = validate_osc_message(message) {
                if let Some(iter) = filter_devices(&broadcast.devices_set[..], &devices) {
                    for device in iter {
                        let device_name = normalize_device_name(&device.name);
                        let _ = match broadcast.command {
                            Command::Vibrate(speed) => {
                                debug!("[{}] adjusting vibration", device_name);
                                device.vibrate(VibrateCommand::Speed(speed))
                            }
                            Command::Stop => {
                                debug!("[{}] stopping", device_name);
                                device.stop()
                            }
                        }.await;
                    }
                }
            }
        }
    }
}

fn filter_devices<'d>(set: &str, devices: &'d evmap::ReadHandle<&str, Device>) -> Option<impl Iterator<Item=evmap::ReadGuard<'d, Device>>> {
    let mut result = Vec::new();

    if let Some(device) = devices.get_one(set) {
        result.push(device);
    } else {
        for (k, _) in devices.read()?.iter() {
            if (set == DEVICES_ALL || k.starts_with(set)) && (*k != DEVICES_LAST) {
                result.push(devices.get_one(k).expect("unexpected"));
            }
        }
    }

    Some(result.into_iter())
}

fn validate_osc_message(message: osc::Message) -> Option<CommandBroadcast> {
    let path = message.addr.split('/').collect::<Vec<&str>>();
    let invalid = |error: &str| {
        warn!("[{}] {}", message.addr, error);
        None::<CommandBroadcast>
    };
    match path.get(1) {
        Some(&"devices") => {
            match path.get(3) {
                Some(&"stop") => {
                    debug!("[{}]", message.addr);
                    Some(CommandBroadcast {
                        devices_set: String::from(path[2]),
                        command: Command::Stop,
                    })
                }
                Some(&"vibrate") => {
                    match path.get(4) {
                        Some(&"speed") => {
                            match message.args {
                                Some(ref message_args) => {
                                    let speed: f64 = match message_args.get(0) {
                                        Some(OscType::Double(x)) => {
                                            *x
                                        }
                                        Some(OscType::Float(x)) => {
                                            (*x).into()
                                        }
                                        _ => {
                                            return invalid(&format!("invalid argument value: {:?}", message_args[0]));
                                        }
                                    };
                                    debug!("[{}] {}", message.addr, speed);
                                    Some(CommandBroadcast {
                                        devices_set: String::from(path[2]),
                                        command: Command::Vibrate(speed),
                                    })
                                }
                                None => invalid("invalid argument value: none")
                            }
                        }
                        _ => invalid("invalid argument name")
                    }
                }
                _ => invalid("invalid command")
            }
        }
        _ => invalid("invalid message")
    }
}

fn validate_osc_listen_url(osc_listen_url: &Url) -> String {
    match osc_listen_url.scheme() {
        "udp" => {}
        _ => {
            unimplemented!("Invalid --osc-listen: only OSC-over-UDP is supported currently");
        }
    }
    let osc_listen_host = osc_listen_url.host().expect("Invalid --osc-listen");
    let osc_listen_port = osc_listen_url.port().expect("Invalid --osc-listen");
    format!("{}:{}", osc_listen_host, osc_listen_port)
}

type Speed = f64;

enum Command {
    Stop,
    Vibrate(Speed),
}

struct CommandBroadcast {
    devices_set: String,
    command: Command,
}


// evmap required Hash trait which was not implemented by ButtplugClientDevice

#[derive(Debug, Eq, Clone, evmap_derive::ShallowCopy)]
struct Device {
    device: Arc<ButtplugClientDevice>
}

impl std::hash::Hash for Device {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.device.name.hash(state);
    }
}

impl PartialEq for Device {
    fn eq(&self, other: &Self) -> bool {
        self.device.eq(&other.device)
    }
}

impl std::ops::Deref for Device {
    type Target = Arc<ButtplugClientDevice>;

    fn deref(&self) -> &Self::Target {
        &self.device
    }
}