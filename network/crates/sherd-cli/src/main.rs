//! Reference client for the sherd daemon: everything a future GUI would do
//! (connect over the local IPC socket, send one `Request`, print the
//! `Response`) in the smallest useful form.

use clap::{Parser, Subcommand};
use interprocess::local_socket::{tokio::prelude::*, tokio::Stream, GenericNamespaced};
use sherd_protocol::{decode_line, encode_line, AutoOutcome, Request, Response, ServerMessage};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Talk to the sherd daemon.
#[derive(Parser)]
#[command(name = "sherd")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Join a nearby sherd network, or host one if none is found. Default
    /// when no subcommand is given.
    Auto,
    /// Report this device's Wi-Fi capability (can it host, or only join?).
    Capability,
    /// Full status: capability + hotspot + station link state.
    Status,
    /// Manually control this device's own hotspot.
    Hotspot {
        #[command(subcommand)]
        action: HotspotAction,
    },
    /// Manually control this device's station (client) connection.
    Station {
        #[command(subcommand)]
        action: StationAction,
    },
}

#[derive(Subcommand)]
enum HotspotAction {
    Start {
        #[arg(long)]
        ssid: String,
        #[arg(long)]
        key: String,
    },
    Stop,
}

#[derive(Subcommand)]
enum StationAction {
    Connect {
        #[arg(long)]
        ssid: String,
        #[arg(long)]
        key: String,
    },
    Disconnect,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    let request = to_request(cli.command.unwrap_or(Command::Auto));

    let response = send_request(request).await?;
    print_response(&response);
    Ok(())
}

fn to_request(command: Command) -> Request {
    match command {
        Command::Auto => Request::Auto,
        Command::Capability => Request::Capability,
        Command::Status => Request::Status,
        Command::Hotspot { action: HotspotAction::Start { ssid, key } } => {
            Request::HotspotStart { ssid, key }
        }
        Command::Hotspot { action: HotspotAction::Stop } => Request::HotspotStop,
        Command::Station { action: StationAction::Connect { ssid, key } } => {
            Request::StationConnect { ssid, key }
        }
        Command::Station { action: StationAction::Disconnect } => Request::StationDisconnect,
    }
}

async fn send_request(request: Request) -> anyhow::Result<Response> {
    let name = sherd_protocol::SOCKET_NAME.to_ns_name::<GenericNamespaced>()?;
    let conn = Stream::connect(name)
        .await
        .map_err(|e| anyhow::anyhow!("could not reach sherd-daemon ({e}) -- is it running?"))?;

    let mut recver = BufReader::new(&conn);
    let mut sender = &conn;
    sender.write_all(encode_line(&request)?.as_bytes()).await?;

    // The daemon may push an `Event` before answering this request; skip
    // past those and wait for the matching `Response`.
    let mut line = String::new();
    loop {
        line.clear();
        let bytes_read = recver.read_line(&mut line).await?;
        if bytes_read == 0 {
            anyhow::bail!("connection closed by sherd-daemon before it answered");
        }
        match decode_line::<ServerMessage>(&line)? {
            ServerMessage::Response(response) => return Ok(response),
            ServerMessage::Event(event) => tracing::debug!(?event, "event while awaiting response"),
        }
    }
}

fn print_response(response: &Response) {
    match response {
        Response::Auto(AutoOutcome::Joined { ssid }) => {
            println!("Joined existing sherd network \"{ssid}\".");
        }
        Response::Auto(AutoOutcome::Hosting { ssid, uplink: Some(uplink) }) => {
            println!("Hosting \"{ssid}\" -- also relaying uplink \"{uplink}\".");
        }
        Response::Auto(AutoOutcome::Hosting { ssid, uplink: None }) => {
            println!("Hosting \"{ssid}\".");
        }
        Response::Auto(AutoOutcome::Unavailable { reason }) => {
            println!("Could not connect: {reason}");
        }
        Response::Capability(report) => {
            println!("Capability: {:?}", report.level);
            println!("  {}", report.detail);
            println!("  (checked via {})", report.checked_via);
        }
        Response::Status(status) => {
            println!(
                "Capability: {:?} -- {}",
                status.capability.level, status.capability.detail
            );
            match &status.hotspot {
                Some(h) => println!("Hotspot: {:?} -- {}", h.state, h.detail),
                None => println!("Hotspot: unknown"),
            }
            match &status.station {
                Some(s) => println!("Station: {:?} -- {}", s.state, s.detail),
                None => println!("Station: unknown"),
            }
        }
        Response::Ok => println!("OK"),
        Response::Error { message } => println!("Error: {message}"),
        Response::NotYetImplemented { feature } => println!("Not implemented yet: {feature}"),
    }
}
