// https://buttplug-developer-guide.docs.buttplug.io/writing-buttplug-applications/api-basics.html#client-server-interaction

use structopt::StructOpt;
use async_std::stream::StreamExt;
use async_std::io::{stdin, ReadExt};
// use nannou_osc as osc;

use buttplug::{
    client::{device::VibrateCommand, ButtplugClient, ButtplugClientEvent},
    connector::{ButtplugRemoteClientConnector, ButtplugWebsocketClientTransport},
    core::messages::serializer::ButtplugClientJSONSerializer,
};

#[derive(StructOpt)]
struct CliArgs {
    #[structopt(long)]
    device: Option<String>,

    #[structopt(long, default_value = "ws://127.0.0.1:12345")]
    intiface_websocket: String,

    #[structopt(long, default_value = "udp://0.0.0.0:9000")]
    osc_listen: String,
}

#[async_std::main]
async fn main() -> anyhow::Result<()> {
    let args = CliArgs::from_args();
    println!("--device {:#?}", args.device);
    println!("--intiface-websocket {:#?}", args.intiface_websocket);
    println!("--osc-listen {:#?}", args.osc_listen);

    let connector = ButtplugRemoteClientConnector::<
        ButtplugWebsocketClientTransport,
        ButtplugClientJSONSerializer,
    >::new(ButtplugWebsocketClientTransport::new_insecure_connector(
        &args.intiface_websocket,
    ));
    println!("connector");

    let client = ButtplugClient::new("Example Client");
    println!("client");

    // https://buttplug-developer-guide.docs.buttplug.io/writing-buttplug-applications/device-enum.html#device-connection-events-and-storage
    // The server could already be running and have devices connected to it. In this case, the Client will emit DeviceAdded events on successful connection.
    // This means you will want to have your event handlers set up BEFORE connecting, in order to catch these messages.
    // You can also check the Devices storage (usually a public collection on your Client instance, like an array or list) after connect to see what devices are there.

    let mut event_stream = client.event_stream();
    println!("event_stream");

    client.connect(connector)
        .await
        .expect("Can't connect to Buttplug Server, exiting!");
    println!("client.connect");

    client.start_scanning().await?;
    println!("client.start_scanning");

    while let Some(event) = event_stream.next().await {
        match event {
            ButtplugClientEvent::DeviceAdded(device) => {
                println!("Device {} Connected!", device.name);
                if args.device.as_ref() == Some(&device.name) {
                    let _ = client.stop_scanning().await;
                    device.vibrate(VibrateCommand::Speed(0.1)).await?;
                    wait_for_input().await;
                    device.stop().await?;
                }
            }
            ButtplugClientEvent::DeviceRemoved(device) => {
                println!("Device {} Removed!", device.name);
                let _ = client.stop_scanning().await;
                client.start_scanning().await?;
            }
            _ => {
                println!("Event: {:#?}", event);
            }
        }
    }

    Ok(())
}

async fn wait_for_input() {
    stdin().bytes().next().await;
}