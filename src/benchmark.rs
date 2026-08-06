//! Reproducible microbenchmark for the reference conversation runtime.
//!
//! This deliberately compares two implementations of the same tiny request/token/done
//! exchange over in-process channels and JSON serialization. It measures reference-runtime
//! overhead, not network or model performance, and makes no claim that Eve is faster.

use crate::Conversation;
use crate::runtime::{RuntimeError, run_memory_demo};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::hint::black_box;
use std::sync::mpsc;
use std::time::Instant;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BenchmarkError {
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
    #[error("benchmark iterations must be greater than zero")]
    InvalidIterations,
    #[error("baseline wire codec error: {0}")]
    Codec(#[from] serde_json::Error),
    #[error("baseline protocol error: {0}")]
    Protocol(String),
    #[error("baseline worker panicked")]
    WorkerPanicked,
}

#[derive(Debug, Clone, Serialize)]
pub struct BenchmarkConfig {
    pub eve_version: &'static str,
    pub iterations: usize,
    pub warmup_iterations: usize,
    pub tokens: usize,
    pub prompt_bytes: usize,
    pub build_profile: &'static str,
    pub target_os: &'static str,
    pub target_arch: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct BenchmarkStats {
    pub min_ns: u64,
    pub median_ns: u64,
    pub mean_ns: u64,
    pub p95_ns: u64,
    pub max_ns: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct BenchmarkReport {
    pub benchmark: &'static str,
    pub config: BenchmarkConfig,
    pub eve_reference: BenchmarkStats,
    pub conventional_baseline: BenchmarkStats,
    pub median_overhead_ratio: f64,
    pub notes: Vec<&'static str>,
}

/// Compare the Eve reference memory runtime with a hand-written JSON protocol.
///
/// Both variants create two worker threads, cross in-process channels, serialize every
/// protocol message to JSON, and complete the same request/token/done exchange. Eve also
/// projects and checks its conversation machines and validates every wire envelope.
pub fn run_reference_benchmark(
    conversation: &Conversation,
    prompt: &str,
    tokens: usize,
    iterations: usize,
    warmup_iterations: usize,
) -> Result<BenchmarkReport, BenchmarkError> {
    if iterations == 0 {
        return Err(BenchmarkError::InvalidIterations);
    }

    for _ in 0..warmup_iterations {
        run_eve_iteration(conversation, prompt, tokens)?;
        run_baseline_iteration(prompt, tokens)?;
    }

    let mut eve_samples = Vec::with_capacity(iterations);
    let mut baseline_samples = Vec::with_capacity(iterations);
    for index in 0..iterations {
        // Alternate execution order to reduce a simple first/second ordering bias.
        if index % 2 == 0 {
            eve_samples.push(measure(|| run_eve_iteration(conversation, prompt, tokens))?);
            baseline_samples.push(measure(|| run_baseline_iteration(prompt, tokens))?);
        } else {
            baseline_samples.push(measure(|| run_baseline_iteration(prompt, tokens))?);
            eve_samples.push(measure(|| run_eve_iteration(conversation, prompt, tokens))?);
        }
    }

    let eve_reference = stats(eve_samples);
    let conventional_baseline = stats(baseline_samples);
    let median_overhead_ratio =
        eve_reference.median_ns as f64 / conventional_baseline.median_ns.max(1) as f64;

    Ok(BenchmarkReport {
        benchmark: "request-token-done/json-channels/v0",
        config: BenchmarkConfig {
            eve_version: env!("CARGO_PKG_VERSION"),
            iterations,
            warmup_iterations,
            tokens,
            prompt_bytes: prompt.len(),
            build_profile: if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
            target_os: std::env::consts::OS,
            target_arch: std::env::consts::ARCH,
        },
        eve_reference,
        conventional_baseline,
        median_overhead_ratio,
        notes: vec![
            "This is a local reference-runtime microbenchmark, not a production performance claim.",
            "Both variants include thread creation, channels, JSON encoding, and JSON decoding in every sample.",
            "Eve additionally includes projection, semantic identity, state-machine checks, and full wire envelopes.",
            "Run the release binary on an otherwise idle machine and compare saved reports, not isolated runs.",
        ],
    })
}

fn run_eve_iteration(
    conversation: &Conversation,
    prompt: &str,
    tokens: usize,
) -> Result<(), BenchmarkError> {
    let report = black_box(run_memory_demo(conversation, prompt, tokens, None)?);
    if !report.client.successful
        || !report.server.successful
        || report.client.tokens.len() != tokens
        || report.server.tokens.len() != tokens
    {
        return Err(BenchmarkError::Protocol(
            "Eve reference iteration did not complete the requested workload".to_string(),
        ));
    }
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum ClientMessage {
    Prompt { text: String },
    Continue,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum ServerMessage {
    Token { id: u64 },
    Done,
}

fn run_baseline_iteration(prompt: &str, tokens: usize) -> Result<(), BenchmarkError> {
    let (client_to_server_tx, client_to_server_rx) = mpsc::channel::<Vec<u8>>();
    let (server_to_client_tx, server_to_client_rx) = mpsc::channel::<Vec<u8>>();
    let prompt = prompt.to_string();
    let expected_prompt = prompt.clone();

    let received_tokens = std::thread::scope(|scope| {
        let server = scope.spawn(move || -> Result<(), BenchmarkError> {
            let message: ClientMessage = receive_json(&client_to_server_rx)?;
            match message {
                ClientMessage::Prompt { text } if text == expected_prompt => {}
                ClientMessage::Prompt { .. } => {
                    return Err(BenchmarkError::Protocol(
                        "baseline server received a different prompt".to_string(),
                    ));
                }
                ClientMessage::Continue => {
                    return Err(BenchmarkError::Protocol(
                        "baseline server expected a prompt".to_string(),
                    ));
                }
            }

            for index in 0..tokens {
                send_json(
                    &server_to_client_tx,
                    &ServerMessage::Token {
                        id: index as u64 + 1,
                    },
                )?;
                if !matches!(
                    receive_json::<ClientMessage>(&client_to_server_rx)?,
                    ClientMessage::Continue
                ) {
                    return Err(BenchmarkError::Protocol(
                        "baseline server expected continue".to_string(),
                    ));
                }
            }
            send_json(&server_to_client_tx, &ServerMessage::Done)
        });

        let client = scope.spawn(move || -> Result<usize, BenchmarkError> {
            send_json(
                &client_to_server_tx,
                &ClientMessage::Prompt { text: prompt },
            )?;
            let mut received = 0;
            loop {
                match receive_json::<ServerMessage>(&server_to_client_rx)? {
                    ServerMessage::Token { id } if id == received as u64 + 1 => {
                        received += 1;
                        send_json(&client_to_server_tx, &ClientMessage::Continue)?;
                    }
                    ServerMessage::Token { id } => {
                        return Err(BenchmarkError::Protocol(format!(
                            "baseline client received token {id} out of order"
                        )));
                    }
                    ServerMessage::Done => break,
                }
            }
            Ok(received)
        });

        let received = client
            .join()
            .map_err(|_| BenchmarkError::WorkerPanicked)??;
        server
            .join()
            .map_err(|_| BenchmarkError::WorkerPanicked)??;
        Ok::<_, BenchmarkError>(received)
    })?;

    if received_tokens != tokens {
        return Err(BenchmarkError::Protocol(format!(
            "baseline client received {received_tokens} tokens; expected {tokens}"
        )));
    }
    black_box(received_tokens);
    Ok(())
}

fn send_json<T: Serialize>(
    sender: &mpsc::Sender<Vec<u8>>,
    value: &T,
) -> Result<(), BenchmarkError> {
    let encoded = serde_json::to_vec(value)?;
    sender
        .send(encoded)
        .map_err(|_| BenchmarkError::Protocol("baseline channel closed during send".to_string()))
}

fn receive_json<T: DeserializeOwned>(
    receiver: &mpsc::Receiver<Vec<u8>>,
) -> Result<T, BenchmarkError> {
    let encoded = receiver.recv().map_err(|_| {
        BenchmarkError::Protocol("baseline channel closed during receive".to_string())
    })?;
    Ok(serde_json::from_slice(&encoded)?)
}

fn measure<F>(operation: F) -> Result<u64, BenchmarkError>
where
    F: FnOnce() -> Result<(), BenchmarkError>,
{
    let start = Instant::now();
    operation()?;
    Ok(start.elapsed().as_nanos().min(u64::MAX as u128) as u64)
}

fn stats(mut samples: Vec<u64>) -> BenchmarkStats {
    samples.sort_unstable();
    let len = samples.len();
    let sum = samples.iter().map(|sample| *sample as u128).sum::<u128>();
    let p95_index = ((len * 95).div_ceil(100)).saturating_sub(1);
    BenchmarkStats {
        min_ns: samples[0],
        median_ns: samples[len / 2],
        mean_ns: (sum / len as u128).min(u64::MAX as u128) as u64,
        p95_ns: samples[p95_index],
        max_ns: samples[len - 1],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example() -> Conversation {
        serde_json::from_str(include_str!("../examples/generate.eveconv.json"))
            .expect("example conversation")
    }

    #[test]
    fn benchmark_executes_both_equivalent_workloads() {
        let report = run_reference_benchmark(&example(), "benchmark", 2, 3, 1).unwrap();
        assert_eq!(report.config.iterations, 3);
        assert_eq!(report.config.tokens, 2);
        assert!(report.eve_reference.median_ns > 0);
        assert!(report.conventional_baseline.median_ns > 0);
        assert!(report.median_overhead_ratio.is_finite());
    }
}
