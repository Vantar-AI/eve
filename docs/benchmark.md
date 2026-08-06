# Reference benchmark

Eve's first benchmark asks a deliberately narrow question: what does the current checked conversation runtime cost relative to a small hand-written protocol?

It is a reproducible microbenchmark, not evidence that Eve is fast. The current reference implementation is expected to lose because it interprets JSON conversation graphs and reconstructs projected machines for every short conversation.

## Workload

Both variants execute the same request/token/done exchange:

1. Create a client worker and a server worker.
2. Send one prompt from client to server.
3. Send the configured number of ordered tokens from server to client, with a continue response after each token.
4. Send a terminal done message.

Both variants include thread creation, in-process channels, JSON encoding, and JSON decoding in every sample. The conventional baseline is a hand-written pair of Rust message loops. Eve additionally includes conversation validation, endpoint projection, semantic identity calculation, full Eve Wire envelopes, and state/sequence checks.

Run an optimized build on an otherwise idle machine:

```bash
cargo run --release --locked -- benchmark \
  --iterations 200 --warmup 20 --tokens 3
```

The CLI emits one JSON report containing the build profile, target, workload parameters, nanosecond samples summarized as minimum/median/mean/p95/maximum, and the median overhead ratio. Warmups are excluded. Measured variants alternate order to reduce a simple first/second ordering bias. There are no timing thresholds in the test suite; tests assert only that both workloads complete correctly.

## Initial result

The checked-in [initial report](../benchmarks/reference-2026-08-06-m4.json) used an Apple M4, Darwin arm64, Rust 1.96.0, 200 measured iterations, 20 warmups, and three tokens:

| Implementation | Median | p95 |
| --- | ---: | ---: |
| Eve reference | 96.5 µs | 172.3 µs |
| Hand-written baseline | 44.8 µs | 88.3 µs |

The Eve median was 2.16× the baseline. That is the honest starting point: the unoptimized semantic machinery adds about 52 µs to this tiny, startup-heavy local exchange on that run. It does not predict networked inference performance, and another machine or run will produce different timings.

## Interpretation and next measurements

This benchmark measures whole-conversation reference overhead. It does not isolate transport latency, steady-state message cost, throughput, allocation, graph compilation, or model execution. It also compares Eve's full envelopes with a smaller baseline encoding, which accurately describes the current implementation but not an optimized wire plan.

Useful next experiments are:

- compile and cache endpoint machines so validation and projection leave the hot path;
- separate one-time conversation startup from steady-state transition cost;
- measure JSON envelope bytes and replace representation overhead only when evidence justifies it;
- compare TCP and QUIC against conventional equivalents under controlled network faults;
- move to a representative AI workload before making any performance claim.

The benchmark succeeds as research even when Eve loses: it gives optimization work a fixed workload and makes regressions visible.
