//! Reproducible microbenchmark for the reference conversation runtime.
//!
//! This deliberately compares two implementations of the same tiny request/token/done
//! exchange over in-process channels and JSON serialization. It measures reference-runtime
//! overhead, not network or model performance, and makes no claim that Eve is faster.

use crate::Conversation;
use crate::plan::{EvePlan, PlanError, PreparedPlan};
use crate::runtime::{
    EndpointMachine, PreparedCompactRoundTrip, RuntimeError, WireEncoding, WireEnvelope,
    run_memory_plan_demo, run_memory_plan_demo_with_encoding,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::json;
use std::hint::black_box;
use std::sync::mpsc;
use std::time::Instant;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BenchmarkError {
    #[error(transparent)]
    Plan(#[from] PlanError),
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
    pub eve_compile: BenchmarkStats,
    pub eve_session_startup: BenchmarkStats,
    pub eve_checked_transition: BenchmarkStats,
    pub eve_compact_checked_transition: BenchmarkStats,
    pub baseline_json_transition: BenchmarkStats,
    pub eve_cold: BenchmarkStats,
    pub eve_warm: BenchmarkStats,
    pub eve_compact_warm: BenchmarkStats,
    pub conventional_baseline: BenchmarkStats,
    pub cold_median_overhead_ratio: f64,
    pub warm_median_overhead_ratio: f64,
    pub compact_warm_median_overhead_ratio: f64,
    pub warm_speedup_over_cold: f64,
    pub compact_speedup_over_reference: f64,
    pub transition_median_overhead_ratio: f64,
    pub compact_transition_median_overhead_ratio: f64,
    pub notes: Vec<&'static str>,
}

/// Compare the Eve reference memory runtime with a hand-written JSON protocol.
///
/// Both variants create two worker threads, cross in-process channels, serialize every
/// protocol message to JSON, and complete the same request/token/done exchange. Cold Eve
/// compiles first; warm Eve reuses a verified plan. Separate samples expose session startup
/// and one checked JSON transition.
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

    let plan = PreparedPlan::compile(conversation)?;
    for _ in 0..warmup_iterations {
        run_compile_iteration(conversation)?;
        run_eve_cold_iteration(conversation, prompt, tokens)?;
        run_eve_warm_iteration(&plan, prompt, tokens)?;
        run_eve_compact_warm_iteration(&plan, prompt, tokens)?;
        run_baseline_iteration(prompt, tokens)?;
        measure_session_startup(&plan)?;
        measure_eve_checked_transition(&plan, prompt)?;
        measure_eve_compact_checked_transition(&plan, prompt)?;
        measure_baseline_json_transition(prompt)?;
    }

    let mut compile_samples = Vec::with_capacity(iterations);
    let mut session_samples = Vec::with_capacity(iterations);
    let mut eve_transition_samples = Vec::with_capacity(iterations);
    let mut eve_compact_transition_samples = Vec::with_capacity(iterations);
    let mut baseline_transition_samples = Vec::with_capacity(iterations);
    let mut cold_samples = Vec::with_capacity(iterations);
    let mut warm_samples = Vec::with_capacity(iterations);
    let mut compact_warm_samples = Vec::with_capacity(iterations);
    let mut baseline_samples = Vec::with_capacity(iterations);
    for index in 0..iterations {
        // Rotate execution order to reduce a simple first/last ordering bias.
        for offset in 0..5 {
            match (index + offset) % 5 {
                0 => compile_samples.push(measure(|| run_compile_iteration(conversation))?),
                1 => cold_samples.push(measure(|| {
                    run_eve_cold_iteration(conversation, prompt, tokens)
                })?),
                2 => warm_samples.push(measure(|| run_eve_warm_iteration(&plan, prompt, tokens))?),
                3 => compact_warm_samples.push(measure(|| {
                    run_eve_compact_warm_iteration(&plan, prompt, tokens)
                })?),
                4 => baseline_samples.push(measure(|| run_baseline_iteration(prompt, tokens))?),
                _ => unreachable!("modulo five is in range"),
            }
        }
    }

    for index in 0..iterations {
        session_samples.push(measure_session_startup(&plan)?);
        for offset in 0..3 {
            match (index + offset) % 3 {
                0 => eve_transition_samples.push(measure_eve_checked_transition(&plan, prompt)?),
                1 => eve_compact_transition_samples
                    .push(measure_eve_compact_checked_transition(&plan, prompt)?),
                2 => baseline_transition_samples.push(measure_baseline_json_transition(prompt)?),
                _ => unreachable!("modulo three is in range"),
            }
        }
    }

    let eve_compile = stats(compile_samples);
    let eve_session_startup = stats(session_samples);
    let eve_checked_transition = stats(eve_transition_samples);
    let eve_compact_checked_transition = stats(eve_compact_transition_samples);
    let baseline_json_transition = stats(baseline_transition_samples);
    let eve_cold = stats(cold_samples);
    let eve_warm = stats(warm_samples);
    let eve_compact_warm = stats(compact_warm_samples);
    let conventional_baseline = stats(baseline_samples);
    let cold_median_overhead_ratio =
        eve_cold.median_ns as f64 / conventional_baseline.median_ns.max(1) as f64;
    let warm_median_overhead_ratio =
        eve_warm.median_ns as f64 / conventional_baseline.median_ns.max(1) as f64;
    let compact_warm_median_overhead_ratio =
        eve_compact_warm.median_ns as f64 / conventional_baseline.median_ns.max(1) as f64;
    let warm_speedup_over_cold = eve_cold.median_ns as f64 / eve_warm.median_ns.max(1) as f64;
    let compact_speedup_over_reference =
        eve_warm.median_ns as f64 / eve_compact_warm.median_ns.max(1) as f64;
    let transition_median_overhead_ratio =
        eve_checked_transition.median_ns as f64 / baseline_json_transition.median_ns.max(1) as f64;
    let compact_transition_median_overhead_ratio = eve_compact_checked_transition.median_ns as f64
        / baseline_json_transition.median_ns.max(1) as f64;

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
        eve_compile,
        eve_session_startup,
        eve_checked_transition,
        eve_compact_checked_transition,
        baseline_json_transition,
        eve_cold,
        eve_warm,
        eve_compact_warm,
        conventional_baseline,
        cold_median_overhead_ratio,
        warm_median_overhead_ratio,
        compact_warm_median_overhead_ratio,
        warm_speedup_over_cold,
        compact_speedup_over_reference,
        transition_median_overhead_ratio,
        compact_transition_median_overhead_ratio,
        notes: vec![
            "This is a local reference-runtime microbenchmark, not a production performance claim.",
            "Every full-workload variant includes thread creation, channels, JSON encoding, and JSON decoding in every sample.",
            "Eve cold includes validation, identity, projection, session startup, state-machine checks, and full wire envelopes.",
            "Eve warm reuses a verified plan but still includes session startup, state-machine checks, and full wire envelopes.",
            "Eve compact warm reuses the same verified plan and exchanges transition ID, sequence, and optional payload instead of repeated semantic strings.",
            "Checked-transition samples exclude session creation and channels; they include two local machine transitions plus reference or compact envelope JSON encoding and decoding.",
            "Run the release binary on an otherwise idle machine and compare saved reports, not isolated runs.",
        ],
    })
}

fn run_compile_iteration(conversation: &Conversation) -> Result<(), BenchmarkError> {
    let plan = black_box(EvePlan::compile(conversation)?);
    if plan.endpoints.len() != 2 {
        return Err(BenchmarkError::Protocol(
            "compiled plan did not contain two endpoints".to_string(),
        ));
    }
    Ok(())
}

fn run_eve_cold_iteration(
    conversation: &Conversation,
    prompt: &str,
    tokens: usize,
) -> Result<(), BenchmarkError> {
    let plan = PreparedPlan::compile(conversation)?;
    run_eve_warm_iteration(&plan, prompt, tokens)
}

fn run_eve_warm_iteration(
    plan: &PreparedPlan,
    prompt: &str,
    tokens: usize,
) -> Result<(), BenchmarkError> {
    let report = black_box(run_memory_plan_demo(plan, prompt, tokens, None)?);
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

fn run_eve_compact_warm_iteration(
    plan: &PreparedPlan,
    prompt: &str,
    tokens: usize,
) -> Result<(), BenchmarkError> {
    let report = black_box(run_memory_plan_demo_with_encoding(
        plan,
        prompt,
        tokens,
        None,
        WireEncoding::Compact,
    )?);
    if !report.client.successful
        || !report.server.successful
        || report.client.tokens.len() != tokens
        || report.server.tokens.len() != tokens
    {
        return Err(BenchmarkError::Protocol(
            "Eve compact iteration did not complete the requested workload".to_string(),
        ));
    }
    Ok(())
}

fn measure_session_startup(plan: &PreparedPlan) -> Result<u64, BenchmarkError> {
    let start = Instant::now();
    let client = EndpointMachine::from_plan(plan, "client")?;
    let server = EndpointMachine::from_plan(plan, "server")?;
    black_box((client.current_state(), server.current_state()));
    Ok(elapsed_ns(start))
}

fn measure_eve_checked_transition(
    plan: &PreparedPlan,
    prompt: &str,
) -> Result<u64, BenchmarkError> {
    let mut client = EndpointMachine::from_plan(plan, "client")?;
    let mut server = EndpointMachine::from_plan(plan, "server")?;
    let start = Instant::now();
    let envelope = client.emit_data(json!({ "text": prompt }))?;
    let encoded = serde_json::to_vec(&envelope)?;
    let decoded: WireEnvelope = serde_json::from_slice(&encoded)?;
    let frame = server.accept(decoded)?;
    black_box(frame);
    Ok(elapsed_ns(start))
}

fn measure_eve_compact_checked_transition(
    plan: &PreparedPlan,
    prompt: &str,
) -> Result<u64, BenchmarkError> {
    let mut client = EndpointMachine::from_plan(plan, "client")?;
    let mut server = EndpointMachine::from_plan(plan, "server")?;
    let compact = PreparedCompactRoundTrip::new(plan);
    let start = Instant::now();
    let envelope = client.emit_data(json!({ "text": prompt }))?;
    let decoded = compact.execute(&envelope)?;
    let frame = server.accept(decoded)?;
    black_box(frame);
    Ok(elapsed_ns(start))
}

fn measure_baseline_json_transition(prompt: &str) -> Result<u64, BenchmarkError> {
    let start = Instant::now();
    let message = ClientMessage::Prompt {
        text: prompt.to_string(),
    };
    let encoded = serde_json::to_vec(&message)?;
    let decoded: ClientMessage = serde_json::from_slice(&encoded)?;
    if !matches!(decoded, ClientMessage::Prompt { text } if text == prompt) {
        return Err(BenchmarkError::Protocol(
            "baseline transition changed the prompt".to_string(),
        ));
    }
    Ok(elapsed_ns(start))
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
    Ok(elapsed_ns(start))
}

fn elapsed_ns(start: Instant) -> u64 {
    start.elapsed().as_nanos().min(u64::MAX as u128) as u64
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
    fn benchmark_executes_all_equivalent_workloads() {
        let report = run_reference_benchmark(&example(), "benchmark", 2, 3, 1).unwrap();
        assert_eq!(report.config.iterations, 3);
        assert_eq!(report.config.tokens, 2);
        assert!(report.eve_compile.median_ns > 0);
        assert!(report.eve_session_startup.median_ns > 0);
        assert!(report.eve_checked_transition.median_ns > 0);
        assert!(report.eve_compact_checked_transition.median_ns > 0);
        assert!(report.baseline_json_transition.median_ns > 0);
        assert!(report.eve_cold.median_ns > 0);
        assert!(report.eve_warm.median_ns > 0);
        assert!(report.eve_compact_warm.median_ns > 0);
        assert!(report.conventional_baseline.median_ns > 0);
        assert!(report.cold_median_overhead_ratio.is_finite());
        assert!(report.warm_median_overhead_ratio.is_finite());
        assert!(report.compact_warm_median_overhead_ratio.is_finite());
        assert!(report.warm_speedup_over_cold.is_finite());
        assert!(report.compact_speedup_over_reference.is_finite());
        assert!(report.transition_median_overhead_ratio.is_finite());
        assert!(report.compact_transition_median_overhead_ratio.is_finite());
    }
}
