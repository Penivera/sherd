//! Reference client for the sherd daemon: everything a future GUI would do
//! (connect over the local IPC socket, send one `Request`, print the
//! `Response`) in the smallest useful form.

use clap::{Parser, Subcommand};
use interprocess::local_socket::{tokio::prelude::*, tokio::Stream, GenericNamespaced};
use platform::{CapabilityLevel, LinkState, LinkStatus};
use protocol::{decode_line, encode_line, Event, Request, Response, ServerMessage, StatusReport};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Talk to the Sherd daemon running on this PC.
#[derive(Parser)]
#[command(name = "sherd")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// What this device is doing right now. The default when no command is
    /// given.
    Status,
    /// Re-run "join a nearby Sherd network / turn on the hotspot" right now,
    /// instead of waiting for the daemon to do it by itself.
    Auto,
    /// Can this device's Wi-Fi host a hotspot, or only join networks?
    Capability,
    /// Manually control this device's own hotspot.
    Hotspot {
        #[command(subcommand)]
        action: HotspotAction,
    },
    /// Manually join or leave a Wi-Fi network.
    Station {
        #[command(subcommand)]
        action: StationAction,
    },
    /// This device's name and permanent ID.
    Whoami,
    /// Other Sherd devices you can reach right now.
    Peers,
    /// Send a text message to a device (see `peers` for its ID).
    Send {
        /// The recipient's device ID, from `sherd peers` (the short form is fine).
        to: String,
        /// The message. Multiple words are joined with spaces, so it
        /// doesn't need quoting: `sherd send 1b13a2c4 hello there`.
        #[arg(required = true, num_args = 1..)]
        body: Vec<String>,
    },
    /// Send a file to a device (see `peers` for its ID).
    SendFile {
        /// The recipient's device ID, from `sherd peers` (the short form is fine).
        to: String,
        /// The file to send.
        path: String,
    },
    /// Past messages and files exchanged with a device.
    History {
        /// The device ID, from `sherd peers` (the short form is fine).
        device_id: String,
    },
    /// Watch messages, files, and devices coming and going, live. Optional:
    /// the daemon receives and saves everything whether or not this is
    /// running. Ctrl+C to stop.
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
async fn main() {
    tracing_subscriber::fmt().with_env_filter(
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
    ).init();

    let cli = Cli::parse();
    let command = cli.command.unwrap_or(Command::Status);

    let result = if matches!(command, Command::Listen) {
        run_listen().await
    } else {
        let confirmation = confirmation_for(&command);
        match send_request(to_request(command)).await {
            Ok(response) => {
                print_response(&response, confirmation);
                Ok(())
            }
            Err(e) => Err(e),
        }
    };

    if let Err(e) = &result {
        eprintln!("{e}");
    }
    pause_if_own_window();
    if result.is_err() {
        std::process::exit(1);
    }
}

/// Double-clicking `sherd.exe` opens a window that closes the instant it
/// finishes -- before the answer can be read. Hold it open in that case.
fn pause_if_own_window() {
    #[cfg(windows)]
    if platform_windows::owns_console_window() {
        eprintln!("\nPress Enter to close this window.");
        let _ = std::io::stdin().read_line(&mut String::new());
    }
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

/// What to print when a command that only answers "OK" succeeds.
fn confirmation_for(command: &Command) -> &'static str {
    match command {
        Command::Send { .. } => "Message sent.",
        Command::SendFile { .. } => "File sent.",
        Command::Hotspot { action: HotspotAction::Start { .. } } => "Hotspot turned on.",
        Command::Hotspot { action: HotspotAction::Stop } => {
            "Hotspot turned off. (The daemon will turn it back on at its next check -- close the \
             daemon to keep it off.)"
        }
        Command::Station { action: StationAction::Connect { .. } } => "Connected.",
        Command::Station { action: StationAction::Disconnect } => "Disconnected.",
        _ => "Done.",
    }
}

async fn connect() -> anyhow::Result<Stream> {
    let name = protocol::SOCKET_NAME.to_ns_name::<GenericNamespaced>()?;
    Stream::connect(name).await.map_err(|_| {
        anyhow::anyhow!(
            "Sherd isn't running on this PC. Start daemon.exe first (right-click it and choose \
             \"Run as administrator\"), then try again."
        )
    })
}

async fn send_request(request: Request) -> anyhow::Result<Response> {
    let conn = connect().await?;
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
            anyhow::bail!("The Sherd daemon closed the connection before answering.");
        }
        match decode_line::<ServerMessage>(&line)? {
            ServerMessage::Response(response) => return Ok(response),
            ServerMessage::Event(event) => tracing::debug!(?event, "event while awaiting response"),
        }
    }
}

/// `sherd listen`: hold the connection open and print events as they
/// arrive, instead of sending one request and exiting.
async fn run_listen() -> anyhow::Result<()> {
    let conn = connect().await?;
    let mut recver = BufReader::new(&conn);

    println!("Watching for messages and files. (Everything is saved even when this isn't open.)");
    println!("Press Ctrl+C to stop.\n");
    let mut line = String::new();
    loop {
        line.clear();
        let bytes_read = recver.read_line(&mut line).await?;
        if bytes_read == 0 {
            anyhow::bail!("The Sherd daemon stopped.");
        }
        match decode_line::<ServerMessage>(&line) {
            Ok(ServerMessage::Event(event)) => print_event(&event),
            Ok(ServerMessage::Response(_)) => {} // we never sent a request
            Err(e) => tracing::debug!("could not parse line from daemon: {e}"),
        }
    }
}

fn print_event(event: &Event) {
    match event {
        Event::MessageReceived { from_device_id, from_display_name, body, attachment } => {
            let who = format!("{from_display_name} ({})", short_device_id(from_device_id));
            if let Some(body) = body {
                println!("{who}: {body}");
            }
            if let Some(a) = attachment {
                println!("{who} sent a file: {} ({}) -- saved to {}", a.name, format_size(a.size_bytes), a.path);
            }
        }
        Event::PeerJoined { device_id, display_name } => {
            println!("  {display_name} ({}) is now reachable.", short_device_id(device_id));
        }
        Event::PeerLeft { device_id, display_name } => {
            println!("  {display_name} ({}) is no longer reachable.", short_device_id(device_id));
        }
        // Link/capability changes are what `sherd status` is for.
        _ => {}
    }
}

fn print_response(response: &Response, confirmation: &str) {
    match response {
        Response::Auto(outcome) => println!("{outcome}"),
        Response::Capability(report) => {
            println!("{}", describe_capability(&report.level));
            println!("  ({})", report.detail);
        }
        Response::Status(status) => print_status(status),
        Response::Ok => println!("{confirmation}"),
        // An older daemon answering a newer CLI can't parse commands it
        // doesn't know yet (seen live after updating one but not the other).
        Response::Error { message } if message.contains("unknown variant") => println!(
            "The Sherd daemon running on this PC is an older version that doesn't know this command. \
             Close its window and start the new daemon.exe (from the same folder as this sherd.exe)."
        ),
        Response::Error { message } => println!("Couldn't do that: {message}"),
        Response::NotYetImplemented { feature } => println!("Not built yet: {feature}"),
        Response::Identity { device_id, display_name } => {
            println!("Name:      {display_name}");
            println!("Device ID: {device_id}");
            println!("(Other people can use just the first part, {}, to send to you.)", short_device_id(device_id));
        }
        Response::Peers(peers) => {
            if peers.is_empty() {
                println!("No other Sherd devices in reach right now.");
                println!("(They appear a few seconds after joining the same Wi-Fi network.)");
            } else {
                println!("{} Sherd device(s) in reach:", peers.len());
                for p in peers {
                    println!(
                        "  {}  {}  (last heard from {})",
                        short_device_id(&p.device_id),
                        p.display_name,
                        format_age(p.last_seen_unix)
                    );
                }
                println!("\nSend with: sherd send <ID> <message>");
            }
        }
        Response::History(entries) => {
            if entries.is_empty() {
                println!("No messages with this device yet.");
            } else {
                for e in entries {
                    let who = if e.direction == "outgoing" { "You" } else { "Them" };
                    let what = match (&e.body, &e.attachment_name, &e.attachment_path) {
                        (Some(body), _, _) => body.clone(),
                        (None, Some(name), Some(path)) => format!("[file: {name}]  {path}"),
                        (None, Some(name), None) => format!("[file: {name}]"),
                        _ => "[empty message]".to_string(),
                    };
                    println!("{:>9}  {who}: {what}", format_age(e.created_at_unix));
                }
            }
        }
    }
}

fn print_status(status: &StatusReport) {
    if !status.device_name.is_empty() {
        println!("This device: {} (ID {})", status.device_name, short_device_id(&status.device_id));
    }

    let hotspot_name = (!status.hotspot_name.is_empty()).then_some(status.hotspot_name.as_str());
    println!("Hotspot:     {}", describe_hotspot(status.hotspot.as_ref(), hotspot_name, &status.capability.level));
    println!("Wi-Fi:       {}", describe_station(status.station.as_ref()));
    if let Some(source) = &status.sharing_internet_from {
        println!("Sharing:     this PC's internet (\"{source}\") with everyone on the mesh");
    }
    println!(
        "Nearby:      {}",
        match status.peer_count {
            0 => "no other Sherd devices in reach".to_string(),
            1 => "1 Sherd device in reach (see `sherd peers`)".to_string(),
            n => format!("{n} Sherd devices in reach (see `sherd peers`)"),
        }
    );
}

fn describe_hotspot(link: Option<&LinkStatus>, name: Option<&str>, level: &CapabilityLevel) -> String {
    if !matches!(level, CapabilityLevel::FullMeshCapable) {
        return "not available -- this device's Wi-Fi can only join networks".to_string();
    }
    match link {
        Some(l) if l.state == LinkState::Up => {
            format!("on (\"{}\")", l.ssid.as_deref().or(name).unwrap_or("unknown name"))
        }
        Some(l) if l.state == LinkState::Starting => "switching on/off...".to_string(),
        Some(_) => "off (the daemon turns it back on automatically)".to_string(),
        None => "unknown".to_string(),
    }
}

fn describe_station(link: Option<&LinkStatus>) -> String {
    match link {
        Some(l) if l.state == LinkState::Up => match &l.ssid {
            Some(ssid) if ssid.starts_with("Sherd") => format!("connected to the Sherd network \"{ssid}\""),
            Some(ssid) => format!("connected to \"{ssid}\""),
            None => "connected".to_string(),
        },
        Some(l) if l.state == LinkState::Starting => "connecting...".to_string(),
        Some(_) => "not connected to any network".to_string(),
        None => "unknown".to_string(),
    }
}

fn describe_capability(level: &CapabilityLevel) -> &'static str {
    match level {
        CapabilityLevel::FullMeshCapable => "This device can host a hotspot and join networks.",
        CapabilityLevel::StationOnly => "This device can join networks, but can't host a hotspot.",
        CapabilityLevel::Unsupported => "No usable Wi-Fi found on this device.",
    }
}

/// A `device_id` is 64 characters -- far too long to read or type. The
/// daemon accepts any unambiguous prefix, so the first 8 are shown.
fn short_device_id(device_id: &str) -> &str {
    &device_id[..device_id.len().min(8)]
}

fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["bytes", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} bytes") } else { format!("{size:.1} {}", UNITS[unit]) }
}

/// "12s ago" style. No date/time dependency in this crate for a
/// chat-style CLI that only needs a rough sense of recency.
fn format_age(unix_seconds: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let age = now - unix_seconds;
    if age < 5 {
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
