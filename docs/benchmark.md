# Reference benchmark

Eve's first benchmark asks a deliberately narrow question: what does the current checked conversation runtime cost relative to a small hand-written protocol?

It is a reproducible microbenchmark, not evidence that Eve is fast. It now distinguishes compilation, plan-backed session creation, one checked transition, cold whole-conversation execution, and warm whole-conversation execution.

## Workload

Both variants execute the same request/token/done exchange:

1. Create a client worker and a server worker.
2. Send one prompt from client to server.
3. Send the configured number of ordered tokens from server to client, with a continue response after each token.
4. Send a terminal done message.

For whole-exchange measurements, both variants include thread creation, in-process channels, JSON encoding, and JSON decoding in every sample. The conventional baseline is a hand-written pair of Rust message loops. Eve additionally uses full Eve Wire envelopes and state/sequence checks. Cold Eve also includes plan compilation; warm Eve reuses one verified plan.

Run an optimized build on an otherwise idle machine:

```bash
cargo run --release --locked -- benchmark \
  --iterations 500 --warmup 50 --tokens 3
```

The CLI emits one JSON report containing the build profile, target, workload parameters, nanosecond samples summarized as minimum/median/mean/p95/maximum, and median ratios. Warmups are excluded. Whole-exchange variants rotate order to reduce a simple first/last ordering bias. There are no timing thresholds in the test suite; tests assert only that every workload completes correctly.

## Plan v0 result

The checked-in [Plan v0 report](../benchmarks/plan-v0-reference-2026-08-07-m4.json) used an Apple M4, Darwin arm64, Rust 1.96.0, 500 measured iterations, 50 warmups, and three tokens:

| Measurement | Median | p95 |
| --- | ---: | ---: |
| Eve plan compilation | 40.0 µs | 40.6 µs |
| Two Eve sessions from plan | 0.125 µs | 0.125 µs |
| Eve checked JSON transition | 2.208 µs | 2.291 µs |
| Baseline JSON transition | 0.250 µs | 0.292 µs |
| Eve cold whole exchange | 114.5 µs | 122.9 µs |
| Eve warm whole exchange | 74.1 µs | 83.0 µs |
| Hand-written whole exchange | 41.8 µs | 50.2 µs |

Plan reuse made Eve's whole exchange 1.55× faster than the cold path. Warm Eve remained 1.77× the baseline, missing the provisional 1.25× target. The isolated checked transition was 8.83× the baseline JSON transition, while creating two sessions took only 125 ns. This points at repeated full-envelope representation as the next optimization target rather than plan or session construction.

The earlier [pre-plan result](../benchmarks/reference-2026-08-06-m4.json) remains checked in for history, but OS scheduling makes isolated runs noisy and the conversation gained additional failure branches between measurements. The values should not be treated as a controlled before/after speedup claim.

## Interpretation and next measurements

The checked-transition measurement creates fresh machines before the timer, then times one client emit, Eve envelope JSON encoding and decoding, and one server accept. It deliberately combines protocol checking and representation cost; it does not yet isolate them. The baseline transition constructs, encodes, decodes, and checks its smaller prompt message.

The benchmark still does not isolate channel transfer, transport latency, throughput, allocation, or model execution. It compares Eve's full envelope with a smaller baseline encoding, which accurately describes the current implementation but not a future specialized wire plan.

Useful next experiments are:

- establish session-level identities once and replace repeated strings with compact transition IDs;
- split state-machine checking from serialization and channel transfer;
- measure JSON envelope bytes before selecting a compact reference encoding;
- compare TCP and QUIC against conventional equivalents under controlled network faults;
- move to a representative AI workload before making any performance claim.

The benchmark succeeds as research even when Eve loses: it gives optimization work a fixed workload and makes regressions visible.
