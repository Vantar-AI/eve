use clap::{Parser, Subcommand, ValueEnum};
use eve::runtime::{
    QuicListener, QuicTransport, TcpTransport, run_generate_client, run_generate_server,
    run_memory_demo, run_quic_demo, run_tcp_demo,
};
use eve::{Conversation, Frame, project, validate, verify_trace};
use serde::de::DeserializeOwned;
use std::fs;
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};

#[derive(Debug, Parser)]
#[command(
    name = "eve",
    version,
    about = "Experimental compiler for graph-native server conversations"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Validate a global Eve conversation.
    Check {
        conversation: PathBuf,
        /// Emit validation diagnostics as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Project a global conversation into one endpoint machine per role.
    Project {
        conversation: PathBuf,
        #[arg(long, default_value = "build/endpoints")]
        out: PathBuf,
    },
    /// Verify that a frame trace follows the global conversation.
    VerifyTrace {
        conversation: PathBuf,
        trace: PathBuf,
    },
    /// Run both projected endpoints using one selectable transport plan.
    Demo {
        #[arg(default_value = "examples/generate.eveconv.json")]
        conversation: PathBuf,
        #[arg(long, value_enum, default_value_t = DemoTransport::Memory)]
        transport: DemoTransport,
        #[arg(
            long,
            default_value = "Explain why the conversation is the computation."
        )]
        prompt: String,
        /// Maximum number of tokens the reference server will emit.
        #[arg(long, default_value_t = 3)]
        tokens: usize,
        /// Ask the client to cancel after receiving this many tokens.
        #[arg(long)]
        cancel_after: Option<usize>,
    },
    /// Serve one projected endpoint over TCP and exit after one conversation.
    Serve {
        #[arg(default_value = "examples/generate.eveconv.json")]
        conversation: PathBuf,
        #[arg(long, default_value = "127.0.0.1:7878")]
        listen: SocketAddr,
        #[arg(long, default_value_t = 3)]
        tokens: usize,
    },
    /// Connect the client endpoint to an Eve TCP server.
    Connect {
        #[arg(default_value = "examples/generate.eveconv.json")]
        conversation: PathBuf,
        #[arg(long, default_value = "127.0.0.1:7878")]
        server: SocketAddr,
        #[arg(
            long,
            default_value = "Explain why the conversation is the computation."
        )]
        prompt: String,
        #[arg(long)]
        cancel_after: Option<usize>,
    },
    /// Serve one projected endpoint over authenticated QUIC.
    ServeQuic {
        #[arg(default_value = "examples/generate.eveconv.json")]
        conversation: PathBuf,
        #[arg(long, default_value = "127.0.0.1:7879")]
        listen: SocketAddr,
        /// Write the generated public certificate here for the client to pin.
        #[arg(long, default_value = "build/eve-quic-cert.der")]
        certificate_out: PathBuf,
        #[arg(long, default_value_t = 3)]
        tokens: usize,
    },
    /// Connect the client endpoint over QUIC using a pinned server certificate.
    ConnectQuic {
        #[arg(default_value = "examples/generate.eveconv.json")]
        conversation: PathBuf,
        #[arg(long, default_value = "127.0.0.1:7879")]
        server: SocketAddr,
        #[arg(long, default_value = "build/eve-quic-cert.der")]
        certificate: PathBuf,
        #[arg(
            long,
            default_value = "Explain why the conversation is the computation."
        )]
        prompt: String,
        #[arg(long)]
        cancel_after: Option<usize>,
    },
}

#[derive(Clone, Debug, ValueEnum)]
enum DemoTransport {
    Memory,
    Tcp,
    Quic,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Command::Check { conversation, json } => {
            let conversation: Conversation = read_json(&conversation)?;
            match validate(&conversation) {
                Ok(()) => println!(
                    "valid conversation {} ({} roles, {} states)",
                    conversation.module.id,
                    conversation.roles.len(),
                    conversation.states.len()
                ),
                Err(errors) if json => {
                    println!("{}", serde_json::to_string_pretty(&errors)?);
                    std::process::exit(1);
                }
                Err(errors) => return Err(Box::new(errors)),
            }
        }
        Command::Project { conversation, out } => {
            let conversation: Conversation = read_json(&conversation)?;
            let endpoints = project(&conversation)?;
            fs::create_dir_all(&out)?;
            for endpoint in endpoints {
                let path = out.join(format!("{}.endpoint.json", endpoint.role));
                fs::write(&path, serde_json::to_vec_pretty(&endpoint)?)?;
                println!("wrote {}", path.display());
            }
        }
        Command::VerifyTrace {
            conversation,
            trace,
        } => {
            let conversation: Conversation = read_json(&conversation)?;
            let frames: Vec<Frame> = read_json(&trace)?;
            let report = verify_trace(&conversation, &frames)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            if !report.complete {
                eprintln!("trace is valid but does not reach a terminal state");
                std::process::exit(2);
            }
        }
        Command::Demo {
            conversation,
            transport,
            prompt,
            tokens,
            cancel_after,
        } => {
            let conversation: Conversation = read_json(&conversation)?;
            let report = match transport {
                DemoTransport::Memory => {
                    run_memory_demo(&conversation, &prompt, tokens, cancel_after)?
                }
                DemoTransport::Tcp => run_tcp_demo(&conversation, &prompt, tokens, cancel_after)?,
                DemoTransport::Quic => run_quic_demo(&conversation, &prompt, tokens, cancel_after)?,
            };
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Command::Serve {
            conversation,
            listen,
            tokens,
        } => {
            let conversation: Conversation = read_json(&conversation)?;
            let listener = TcpListener::bind(listen)?;
            println!("Eve server listening on {listen}");
            let (stream, peer) = listener.accept()?;
            println!("accepted Eve endpoint {peer}");
            let mut transport = TcpTransport::from_stream(stream)?;
            let report = run_generate_server(&conversation, &mut transport, tokens)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Command::Connect {
            conversation,
            server,
            prompt,
            cancel_after,
        } => {
            let conversation: Conversation = read_json(&conversation)?;
            let mut transport = TcpTransport::connect(server)?;
            let report = run_generate_client(&conversation, &mut transport, &prompt, cancel_after)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Command::ServeQuic {
            conversation,
            listen,
            certificate_out,
            tokens,
        } => {
            let conversation: Conversation = read_json(&conversation)?;
            let listener = QuicListener::bind(listen)?;
            if let Some(parent) = certificate_out.parent()
                && !parent.as_os_str().is_empty()
            {
                fs::create_dir_all(parent)?;
            }
            fs::write(&certificate_out, listener.certificate_der())?;
            println!(
                "Eve QUIC server listening on {} (certificate: {})",
                listener.local_addr()?,
                certificate_out.display()
            );
            let mut transport = listener.accept()?;
            let report = run_generate_server(&conversation, &mut transport, tokens)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Command::ConnectQuic {
            conversation,
            server,
            certificate,
            prompt,
            cancel_after,
        } => {
            let conversation: Conversation = read_json(&conversation)?;
            let trusted_certificate = fs::read(certificate)?;
            let mut transport = QuicTransport::connect(server, &trusted_certificate)?;
            let report = run_generate_client(&conversation, &mut transport, &prompt, cancel_after)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
    }
    Ok(())
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, Box<dyn std::error::Error>> {
    let input = fs::read(path)?;
    Ok(serde_json::from_slice(&input)?)
}
