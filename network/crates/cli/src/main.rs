//! Reference client for the sherd daemon: everything a future GUI would do
//! (connect over the local IPC socket, send one `Request`, print the
//! `Response`) in the smallest useful form.

use clap::{Parser, Subcommand};
use interprocess::local_socket::{tokio::prelude::*, tokio::Stream, GenericNamespaced};
use protocol::{decode_line, encode_line, AutoOutcome, Event, Request, Response, ServerMessage};
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
    /// Show this device's own persistent ID and display name.
    Whoami,
    /// List other sherd devices currently reachable on this Wi-Fi network.
    Peers,
    /// Send a text message to a device (see `peers` for its ID).
    Send {
        /// The recipient's device ID, from `sherd peers`.
        to: String,
        /// The message text. Multiple words are joined with spaces, so it
        /// doesn't need quoting: `sherd send <id> hello there`.
        #[arg(required = true, num_args = 1..)]
        body: Vec<String>,
    },
    /// Send a file to a device (see `peers` for its ID).
    SendFile {
        /// The recipient's device ID, from `sherd peers`.
        to: String,
        /// Local path of the file to send.
        path: String,
    },
    /// Show past messages exchanged with a device.
    History {
        /// The device ID, from `sherd peers`.
        device_id: String,
    },
    /// Stay connected and print messages/files as they arrive. Ctrl+C to
    /// stop.
    Listen,
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
    let command = cli.command.unwrap_or(Command::Auto);

    if matches!(command, Command::Listen) {
        return run_listen().await;
    }

    let response = send_request(to_request(command)).await?;
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
        Command::Whoami => Request::Identity,
        Command::Peers => Request::Peers,
        Command::Send { to, body } => Request::SendMessage { to, body: body.join(" ") },
        Command::SendFile { to, path } => Request::SendFile { to, path },
        Command::History { device_id } => Request::History { device_id },
        Command::Listen => unreachable!("handled in main() before to_request is called"),
    }
}

/// `sherd listen`: hold the connection open and print events (mainly
/// `MessageReceived`) as they arrive, instead of sending one request and
/// exiting like every other subcommand.
async fn run_listen() -> anyhow::Result<()> {
    let name = protocol::SOCKET_NAME.to_ns_name::<GenericNamespaced>()?;
    let conn = Stream::connect(name)
        .await
        .map_err(|e| anyhow::anyhow!("could not reach sherd-daemon ({e}) -- is it running?"))?;
    let mut recver = BufReader::new(&conn);

    println!("Listening for incoming messages and files. Press Ctrl+C to stop.");
    let mut line = String::new();
    loop {
        line.clear();
        let bytes_read = recver.read_line(&mut line).await?;
        if bytes_read == 0 {
            anyhow::bail!("connection closed by sherd-daemon");
        }
        match decode_line::<ServerMessage>(&line) {
            Ok(ServerMessage::Event(event)) => print_event(&event),
            Ok(ServerMessage::Response(_)) => {} // we never sent a request
            Err(e) => tracing::debug!("could not parse line from daemon: {e}"),
        }
    }
}

fn print_event(event: &Event) {
    if let Event::MessageReceived { from_device_id, from_display_name, body, attachment } = event {
        let short_id = short_device_id(from_device_id);
        if let Some(body) = body {
            println!("[{from_display_name} ({short_id})] {body}");
        }
        if let Some(a) = attachment {
            println!(
                "[{from_display_name} ({short_id})] sent a file: {} ({} bytes) -- saved to {}",
                a.name, a.size_bytes, a.path
            );
        }
    }
    // Other event kinds (link/capability/auto-connect changes) are covered
    // by `sherd status`; `listen` is specifically for messages.
}

async fn send_request(request: Request) -> anyhow::Result<Response> {
    let name = protocol::SOCKET_NAME.to_ns_name::<GenericNamespaced>()?;
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
        Response::Identity { device_id, display_name } => {
            println!("Display name: {display_name}");
            println!("Device ID:    {device_id}");
        }
        Response::Peers(peers) => {
            if peers.is_empty() {
                println!("No other sherd devices seen recently.");
            } else {
                for p in peers {
                    println!(
                        "{}  {}  ({}, last seen {})",
                        short_device_id(&p.device_id),
                        p.display_name,
                        p.addr,
                        format_unix(p.last_seen_unix)
                    );
                }
            }
        }
        Response::History(entries) => {
            if entries.is_empty() {
                println!("No messages with this device yet.");
            } else {
                for e in entries {
                    let arrow = if e.direction == "outgoing" { "->" } else { "<-" };
                    let what = match (&e.body, &e.attachment_name) {
                        (Some(body), _) => body.clone(),
                        (None, Some(name)) => format!("[file: {name}]"),
                        (None, None) => "[empty message]".to_string(),
                    };
                    println!("{} {arrow} {what}  ({}, {})", format_unix(e.created_at_unix), e.status, e.direction);
                }
            }
        }
    }
}

/// A `device_id` is a 64-character hex string (a full Ed25519 public key) --
/// far too long to read or type comfortably. Shown in full only by
/// `whoami`/`send`/`send-file`, which need the exact value; everywhere else
/// (`peers`, `history`, `listen`) a short prefix is enough to tell devices
/// apart at a glance.
fn short_device_id(device_id: &str) -> &str {
    &device_id[..device_id.len().min(8)]
}

fn format_unix(unix_seconds: i64) -> String {
    // No date/time-formatting dependency in this crate; a raw offset from
    // now reads fine for a chat-style CLI ("12s ago") without pulling one
    // in just for this.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let age = now - unix_seconds;
    if age < 0 {
        "just now".to_string()
    } else if age < 60 {
        format!("{age}s ago")
    } else if age < 3600 {
        format!("{}m ago", age / 60)
    } else if age < 86400 {
        format!("{}h ago", age / 3600)
    } else {
        format!("{}d ago", age / 86400)
    }
}
