use clap::{Parser, Subcommand};
use eve::{Conversation, Frame, project, validate, verify_trace};
use serde::de::DeserializeOwned;
use std::fs;
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
    }
    Ok(())
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, Box<dyn std::error::Error>> {
    let input = fs::read(path)?;
    Ok(serde_json::from_slice(&input)?)
}
