use std::{path::PathBuf, sync::Arc};

use clap::{Parser, Subcommand};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use sherd_vm_core::{
    config::{OsKind, SherdVmConfig},
    ipc,
    providers::CreateVmOpts,
    service::VmService,
    stream::InputEvent,
    vm::VmManager,
};

#[derive(Parser)]
#[command(name = "sherd-vm", version, about = "Sherd VM integration — Solari Linux + Windows providers")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Provision a new VM (Linux via Solari, Windows via stub/pluggable)
    Create {
        #[arg(long, default_value = "linux")]
        os: String,
        #[arg(long)]
        template: Option<String>,
        #[arg(long)]
        resolution: Option<String>,
        #[arg(long)]
        cpu: Option<u8>,
        #[arg(long)]
        mem: Option<u32>,
        #[arg(long)]
        timeout_ms: Option<u64>,
        #[arg(long, default_value = "pause")]
        lifecycle: String,
        #[arg(long)]
        from_snapshot: Option<String>,
    },
    /// Get session info
    Get {
        session_id: String,
        #[arg(long)]
        os: Option<String>,
    },
    /// Get VM status (health + stream_url)
    Status {
        session_id: String,
    },
    /// Destroy a VM (idempotent)
    Destroy {
        session_id: String,
    },
    /// Pause a VM (frees concurrency slot)
    Pause {
        session_id: String,
    },
    /// Resume a paused VM
    Resume {
        session_id: String,
    },
    /// Run a command inside the VM
    Exec {
        session_id: String,
        cmd: String,
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Upload a local file to the VM
    Upload {
        session_id: String,
        local: PathBuf,
        remote: String,
    },
    /// Download a remote file from the VM
    Download {
        session_id: String,
        remote: String,
        local: PathBuf,
    },
    /// List tracked sessions (in-memory)
    List,
    /// Get stream URL for a session
    Stream {
        session_id: String,
    },
    /// Capture screenshot (base64 preview or save to file)
    Screenshot {
        session_id: String,
        #[arg(long, default_value = "png")]
        format: String,
        #[arg(long)]
        quality: Option<u8>,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Send an input event (mouse/keyboard)
    Input {
        session_id: String,
        #[arg(long)]
        kind: String, // mouse_move, mouse_click, key_type, key_press
        #[arg(long)]
        x: Option<u32>,
        #[arg(long)]
        y: Option<u32>,
        #[arg(long)]
        button: Option<String>,
        #[arg(long)]
        text: Option<String>,
        #[arg(long)]
        keys: Option<String>, // comma-separated
        #[arg(long, default_value_t = false)]
        humanize: bool,
    },
    /// Health check for a VM
    Health {
        session_id: String,
    },
    /// Run as IPC daemon (sherd-vm.sock) + HTTP bridge
    Serve {
        #[arg(long)]
        socket: Option<String>,
        #[arg(long, default_value = "8765")]
        http_port: u16,
        #[arg(long, default_value_t = false)]
        no_http: bool,
    },
    /// Check auth token via SHERD_AUTH_URL
    AuthCheck {
        token: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let cli = Cli::parse();

    // Serve doesn't require SOLARI_API_KEY at startup — it will error on create if missing
    if let Commands::Serve { socket, http_port, no_http } = cli.command {
        return serve(socket, http_port, no_http).await;
    }

    // For other commands, try IPC first if daemon is running, else direct
    // For MVP, run direct via VmManager (requires SOLARI_API_KEY)
    let config = match SherdVmConfig::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("config error: {} (set SOLARI_API_KEY=slr_live_...)", e);
            std::process::exit(1);
        }
    };
    let manager = Arc::new(VmManager::new(&config)?);
    let service = VmService::new(manager);

    match cli.command {
        Commands::Create { os, template, resolution, cpu, mem, timeout_ms, lifecycle, from_snapshot } => {
            let os_kind: OsKind = os.parse().map_err(|e: String| anyhow::anyhow!(e))?;
            let opts = CreateVmOpts {
                os: os_kind,
                template,
                resolution,
                cpu,
                mem_mb: mem,
                timeout_ms,
                lifecycle: Some(lifecycle),
                from_snapshot,
                volumes: None,
            };
            let sess = service.create(opts).await?;
            println!("{}", serde_json::to_string_pretty(&sess)?);
            if let Some(url) = sess.stream_url {
                println!("stream_url: {}", url);
            }
        }
        Commands::Get { session_id, os: _ } => {
            let sess = service.get(&session_id).await?;
            println!("{}", serde_json::to_string_pretty(&sess)?);
        }
        Commands::Status { session_id } => {
            let st = service.status(&session_id).await?;
            println!("{}", serde_json::to_string_pretty(&st)?);
        }
        Commands::Destroy { session_id } => {
            service.destroy(&session_id).await?;
            println!("destroyed {}", session_id);
        }
        Commands::Pause { session_id } => {
            service.pause(&session_id).await?;
            println!("paused {}", session_id);
        }
        Commands::Resume { session_id } => {
            let sess = service.resume(&session_id).await?;
            println!("{}", serde_json::to_string_pretty(&sess)?);
        }
        Commands::Exec { session_id, cmd, args } => {
            let out = service.exec(&session_id, &cmd, args).await?;
            println!("exit_code: {}", out.exit_code);
            println!("stdout:\n{}", out.stdout);
            if !out.stderr.is_empty() {
                eprintln!("stderr:\n{}", out.stderr);
            }
            if out.exit_code != 0 {
                std::process::exit(out.exit_code);
            }
        }
        Commands::Upload { session_id, local, remote } => {
            let bytes = tokio::fs::read(&local).await?;
            service.upload(&session_id, &remote, &bytes).await?;
            println!("uploaded {} -> {} ({} bytes)", local.display(), remote, bytes.len());
        }
        Commands::Download { session_id, remote, local } => {
            let mgr = service.manager();
            let bytes = mgr.fs_read(&session_id, &remote).await?;
            if let Some(parent) = local.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::write(&local, &bytes).await?;
            println!("downloaded {} -> {} ({} bytes)", remote, local.display(), bytes.len());
        }
        Commands::List => {
            let sessions = service.list().await;
            println!("{}", serde_json::to_string_pretty(&sessions)?);
        }
        Commands::Stream { session_id } => {
            let url = service.stream_url(&session_id).await?;
            println!("{}", url);
        }
        Commands::Screenshot { session_id, format, quality, out } => {
            let bytes = service.screenshot(&session_id, &format, quality).await?;
            if let Some(path) = out {
                tokio::fs::write(&path, &bytes).await?;
                println!("screenshot saved to {} ({} bytes, format={})", path.display(), bytes.len(), format);
            } else {
                let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes);
                println!("data:image/{};base64,{}", format, &b64[..b64.len().min(120)]);
                println!("... ({} bytes total, use --out to save)", bytes.len());
            }
        }
        Commands::Input { session_id, kind, x, y, button, text, keys, humanize } => {
            let event = parse_input(kind, x, y, button, text, keys, humanize)?;
            service.send_input(&session_id, event).await?;
            println!("input sent");
        }
        Commands::Health { session_id } => {
            let h = service.health(&session_id).await?;
            println!("{}", serde_json::to_string_pretty(&h)?);
        }
        Commands::Serve { .. } => unreachable!(),
        Commands::AuthCheck { token } => {
            let auth_url = std::env::var("SHERD_AUTH_URL")
                .or_else(|_| std::env::var("VITE_API_BASE_URL"))
                .unwrap_or_else(|_| "http://localhost:8000".into());
            let url = format!("{}/auth/me", auth_url.trim_end_matches('/'));
            let client = reqwest::Client::new();
            let resp = client.get(&url).header("Authorization", format!("Bearer {}", token)).send().await?;
            if resp.status().is_success() {
                let body: serde_json::Value = resp.json().await?;
                println!("{}", serde_json::to_string_pretty(&body)?);
            } else {
                eprintln!("auth failed: {}", resp.status());
                std::process::exit(1);
            }
        }
    }
    Ok(())
}

fn parse_input(
    kind: String,
    x: Option<u32>,
    y: Option<u32>,
    button: Option<String>,
    text: Option<String>,
    keys: Option<String>,
    humanize: bool,
) -> anyhow::Result<InputEvent> {
    use sherd_vm_core::stream::{InputEvent as IE, InputKind};
    let k = match kind.as_str() {
        "mouse_move" | "move" => InputKind::MouseMove,
        "mouse_click" | "click" => InputKind::MouseClick,
        "mouse_down" | "down" => InputKind::MouseDown,
        "mouse_up" | "up" => InputKind::MouseUp,
        "mouse_scroll" | "scroll" => InputKind::MouseScroll,
        "key_type" | "type" => InputKind::KeyType,
        "key_press" | "press" => InputKind::KeyPress,
        "key_down" => InputKind::KeyDown,
        "key_up" => InputKind::KeyUp,
        _ => anyhow::bail!("unknown input kind: {} (expected mouse_move|mouse_click|key_type|key_press)", kind),
    };
    Ok(IE {
        kind: k,
        x,
        y,
        button,
        keys: keys.map(|s| s.split(',').map(|v| v.trim().to_string()).collect()),
        text,
        humanize: Some(humanize),
        scroll_delta: None,
    })
}

async fn serve(socket: Option<String>, http_port: u16, no_http: bool) -> anyhow::Result<()> {
    let config = SherdVmConfig::from_env().unwrap_or_else(|e| {
        eprintln!("warning: {} — serve will error on create until SOLARI_API_KEY is set", e);
        SherdVmConfig {
            solari: sherd_vm_core::config::SolariConfig {
                api_key: "missing".into(),
                base_url: "https://api.getsolari.com".into(),
                region: "us-west".into(),
                timeout_ms: 90_000,
                max_retries: 5,
                default_template: "default".into(),
                default_resolution: "1280x720".into(),
                default_cpu: 2,
                default_mem_mb: 4096,
                default_timeout_ms: 15 * 60 * 1000,
                default_lifecycle: "pause".into(),
            },
            auth_url: "http://localhost:8000".into(),
            ipc_socket: "sherd-ipc.sock".into(),
            vm_socket: socket.clone().unwrap_or_else(|| "sherd-vm.sock".into()),
            watchdog_interval: std::time::Duration::from_secs(15),
        }
    });
    let auth_url = config.auth_url.clone();
    let manager = Arc::new(VmManager::new(&config).unwrap_or_else(|e| {
        error!("failed to init VmManager: {} — using stub", e);
        let linux: Arc<dyn sherd_vm_core::providers::VmProvider> = Arc::new(sherd_vm_core::providers::windows::WindowsProvider::new());
        let windows: Arc<dyn sherd_vm_core::providers::VmProvider> = Arc::new(sherd_vm_core::providers::windows::WindowsProvider::new());
        VmManager::with_providers(linux, windows)
    }));
    let service = Arc::new(VmService::new(manager));
    if no_http {
        info!(socket = ?socket, "starting sherd-vm IPC daemon (no HTTP)");
        return ipc::serve(service, socket).await;
    }
    let http_service = Arc::clone(&service);
    let http_addr = format!("127.0.0.1:{}", http_port).parse().unwrap();
    let http_auth = auth_url.clone();
    tokio::spawn(async move {
        if let Err(e) = sherd_vm_core::http::serve_http(http_service, http_addr, http_auth).await {
            error!("HTTP bridge failed: {}", e);
        }
    });
    info!(socket = ?socket, http_port, "starting sherd-vm IPC daemon + HTTP bridge");
    ipc::serve(service, socket).await
}
