# Reference benchmark

Eve's first benchmark asks a deliberately narrow question: what does the current checked conversation runtime cost relative to a small hand-written protocol?

It is a reproducible microbenchmark, not evidence that Eve is fast. It distinguishes compilation, plan-backed session creation, reference and compact checked transitions, cold whole-conversation execution, reference and compact warm execution, and a hand-written baseline.

## Workload

All variants execute the same request/token/done exchange:

1. Create a client worker and a server worker.
2. Send one prompt from client to server.
3. Send the configured number of ordered tokens from server to client, with a continue response after each token.
4. Send a terminal done message.

For whole-exchange measurements, every variant includes thread creation, in-process channels, JSON encoding, and JSON decoding in every sample. The conventional baseline is a hand-written pair of Rust message loops. Reference Eve additionally uses self-describing Eve Wire envelopes and state/sequence checks. Compact Eve performs the same checks while exchanging a transition ID, sequence, and optional payload. Cold Eve includes plan compilation; both warm paths reuse one verified plan.

Run an optimized build on an otherwise idle machine:

```bash
cargo run --release --locked -- benchmark \
  --iterations 500 --warmup 50 --tokens 3
```

The CLI emits one JSON report containing the build profile, target, workload parameters, nanosecond samples summarized as minimum/median/mean/p95/maximum, and median ratios. Warmups are excluded. Whole-exchange variants rotate order to reduce a simple first/last ordering bias. There are no timing thresholds in the test suite; tests assert only that every workload completes correctly.

## Compact Eve Wire result

The checked-in [compact-wire report](../benchmarks/compact-wire-v0-reference-2026-08-07-m4.json) used an Apple M4, Darwin arm64, Rust 1.96.0, 500 measured iterations, 50 warmups, and three tokens:

| Measurement | Median | p95 |
| --- | ---: | ---: |
| Eve plan compilation | 51.6 µs | 66.8 µs |
| Two Eve sessions from plan | 0.125 µs | 0.167 µs |
| Eve reference checked transition | 2.417 µs | 2.500 µs |
| Eve compact checked transition | 1.833 µs | 1.917 µs |
| Baseline JSON transition | 0.250 µs | 0.292 µs |
| Eve cold whole exchange | 145.9 µs | 235.5 µs |
| Eve reference warm exchange | 92.5 µs | 166.8 µs |
| Eve compact warm exchange | 83.6 µs | 154.6 µs |
| Hand-written whole exchange | 55.2 µs | 112.5 µs |

Compact encoding made the isolated checked Eve transition 1.32× faster than the reference envelope and the complete warm exchange 1.11× faster. Median overhead versus the hand-written whole exchange fell from 1.68× to 1.51×, still missing the provisional 1.25× target. The compact checked transition remained 7.33× the smaller baseline JSON transition, while creating two sessions took only 125 ns.

The earlier [Plan v0](../benchmarks/plan-v0-reference-2026-08-07-m4.json) and [pre-plan](../benchmarks/reference-2026-08-06-m4.json) results remain checked in for history. OS scheduling makes isolated runs noisy and the benchmark implementation changed between them, so only reference and compact samples within the current report are treated as a controlled comparison.

## Interpretation and next measurements

The checked-transition measurement creates fresh machines and prepares the compact codec before the timer, then times one client emit, envelope JSON encoding and decoding, and one server accept. It deliberately combines protocol checking and representation cost; it does not yet isolate them. The baseline transition constructs, encodes, decodes, and checks its smaller prompt message.

The benchmark still does not isolate channel transfer, transport latency, throughput, allocation, lookup, or model execution. Compact wire is still JSON and reconstructs owned semantic strings before checking, so this is not a lower bound for a binary or zero-copy implementation.

Useful next experiments are:

- add an authenticated session handshake that binds peers to the same plan identity;
- split state-machine checking, transition lookup, allocation, serialization, and channel transfer;
- compare the compact JSON experiment with a schema-driven binary payload codec;
- compare TCP and QUIC against conventional equivalents under controlled network faults;
- move to a representative AI workload before making any performance claim.

The benchmark succeeds as research even when Eve loses: it gives optimization work a fixed workload and makes regressions visible.
