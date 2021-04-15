use std::sync::Arc;
use async_std::stream::StreamExt;
use async_std::task::block_on;
use structopt::StructOpt;
use url::Url;
use nannou_osc as osc;
use nannou_osc::rosc::OscType;
use buttplug::{
    client::{ButtplugClient, ButtplugClientDevice, ButtplugClientEvent, ButtplugClientError,
             device::VibrateCommand},
    connector::{ButtplugRemoteClientConnector, ButtplugWebsocketClientTransport},
    core::messages::serializer::ButtplugClientJSONSerializer,
};

#[derive(StructOpt)]
struct CliArgs {
    #[structopt(long, default_value = "ws://127.0.0.1:12345")]
    intiface_connect: Url,

    #[structopt(long, default_value = "udp://0.0.0.0:9000")]
    osc_listen: Url,
}

#[async_std::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_ansi(false).init();

    let args = CliArgs::from_args();
    // println!("--intiface-connect {:#?}", args.intiface_connect.as_str());
    // println!("--osc-listen {:#?}", args.osc_listen.as_str());

    let osc_listen_host_port = validate_osc_listen_url(&args.osc_listen);
    let (devices_r, mut devices_w) = evmap::new();
    std::thread::spawn(move || {
        osc_listen(&osc_listen_host_port, devices_r);
    });

    // https://buttplug-developer-guide.docs.buttplug.io/writing-buttplug-applications/device-enum.html#device-connection-events-and-storage
    // > The server could already be running and have devices connected to it. In this case, the Client will emit DeviceAdded events on successful connection.
    // > This means you will want to have your event handlers set up BEFORE connecting, in order to catch these messages.

    let client = ButtplugClient::new("buttplug-osc");
    let mut event_stream = client.event_stream();
    let event_loop = async {
        while let Some(event) = event_stream.next().await {
            match event {
                ButtplugClientEvent::DeviceAdded(device) => {
                    // TODO: multiple devices support?
                    // let name = Box::leak(device.name.clone().into_boxed_str());
                    // devices_w.update(name, Device { device: device.clone() });

                    devices_w.update("last", Device { device: device.clone() });
                    devices_w.refresh();
                    println!("[{}] added", device.name);
                }
                ButtplugClientEvent::DeviceRemoved(device) => {
                    println!("[{}] removed", device.name);
                    // rescanning, maybe a temporary disconnect
                    let _ = client.stop_scanning().await;
                    let _ = client.start_scanning().await;
                }
                ButtplugClientEvent::ServerDisconnect => {
                    // TODO: didn't work, need a new instance of client
                    // loop {
                    //     let e = client.disconnect().await;
                    //     println!("{:#?}", e);
                    //     let result =
                    //         intiface_connect(&client, args.intiface_connect.as_str()).await;
                    //     if result.is_ok() {
                    //         break;
                    //     } else {
                    //         println!("{:#?}", result)
                    //     }
                    //     println!("Reconnecting to server...");
                    // }
                    panic!();
                }
                _ => {}
            }
        };
        Ok::<(), anyhow::Error>(())
    };

    intiface_connect(&client, args.intiface_connect.as_str()).await?;
    client.start_scanning().await?;
    event_loop.await?;

    Ok(())
}

async fn intiface_connect(client: &ButtplugClient, address: &str) -> anyhow::Result<(), ButtplugClientError> {
    let connector = ButtplugRemoteClientConnector::<
        ButtplugWebsocketClientTransport,
        ButtplugClientJSONSerializer,
    >::new(ButtplugWebsocketClientTransport::new_insecure_connector(address));
    client.connect(connector).await
}

fn osc_listen(host_port: &str, devices: evmap::ReadHandle<&str, Device>) {
    let rx = osc::Receiver::bind_to(host_port).expect("Invalid --osc-listen: couldn't bind socket");
    // let iter = rx.iter();
    for packet in rx.iter() {
        let messages = packet.0.into_msgs();
        for message in messages {
            // TODO: per-device async queues?
            if let Some(command) = validate_osc_message(message) {
                match command {
                    Command::Vibrate(device_name, params) => {
                        devices.get_one(&device_name[..]).map(|device| {
                            println!("[{}] adjusting vibration", device.name);
                            block_on(device.vibrate(params))
                        });
                    }
                }
            }
        }
    }
}

fn validate_osc_message(message: osc::Message) -> Option<Command> {
    // TODO: extract device name to control multiple devices?
    // instead of just the last one connected
    match &message.addr[..] {
        "/vibrate/speed" => {
            match message.args {
                Some(args) => {
                    let speed: f64 = match args[0] {
                        OscType::Double(x) => {
                            x
                        }
                        OscType::Float(x) => {
                            x.into()
                        }
                        _ => {
                            println!("[{}] invalid argument: {:?}", message.addr, args[0]);
                            return None;
                        }
                    };
                    println!("[{}] {}", message.addr, speed);
                    Some(Command::Vibrate(DeviceName::from("last"), VibrateCommand::Speed(speed)))
                }
                None => {
                    println!("[{}] absent argument", message.addr);
                    None
                }
            }
        }
        _ => {
            println!("[{}] invalid command", message.addr);
            None
        }
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

type DeviceName = String;

enum Command {
    Vibrate(DeviceName, VibrateCommand)
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