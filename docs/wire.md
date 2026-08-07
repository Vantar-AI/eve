# Eve Wire v0

Eve Wire carries conversation transitions between plan-backed endpoint machines. The runtime now has two encodings with identical semantics:

- `reference` sends a self-describing envelope containing the wire version, conversation, semantic identity, state, sequence, and complete frame;
- `compact` sends only a compiled transition ID, sequence, and optional data payload.

Use the compact path with a verified Eve Plan:

```bash
cargo run -- compile examples/generate.eveconv.json
cargo run -- run-plan build/generate.eveplan.json \
  --wire compact --transport quic --tokens 3
```

The same `--wire compact` option works with memory and TCP.

## Compiled transition dictionary

Compilation projects both endpoints, deduplicates their matching send/receive actions, sorts the resulting semantic transitions deterministically, and assigns dense `u16` IDs beginning at one. The dictionary is covered by `plan_identity` and stored in `wire.transitions`.

For the example conversation, transition `7` means:

```json
{
  "id": 7,
  "state": "start",
  "op": "data",
  "from": "client",
  "to": "server",
  "message": "prompt"
}
```

The reference envelope repeats those semantics on every prompt. Compact Eve Wire sends:

```json
{"t":7,"q":0,"p":{"text":"hello"}}
```

For that payload, compact JSON is 34 bytes versus 279 bytes for the reference envelope, an 88% reduction before transport framing. The ratio varies with payload size. Here `t` is the transition ID, `q` is the monotonic sequence, and optional `p` is present only for data payloads. Select and cancel transitions carry no payload. Local fault observations are not transmitted.

The receiver resolves `t` through its verified plan, reconstructs the complete semantic frame and state, then runs the same conversation, identity, sequence, and endpoint-action checks used by the reference path. Execution reports and traces therefore remain representation-independent. Tests require reference and compact sessions over memory, TCP, and QUIC to produce the same semantic trace.

## Trust boundary

Compact mode currently assumes both peers already hold the same verified plan. The demo establishes this out of band by sharing one `PreparedPlan`. Independent-process negotiation, plan-identity exchange, downgrade rules, authorization, and persistent peer identity are not implemented yet. Until that handshake exists, a deployment must pin the expected plan through its own trusted control plane.

The encoding is still JSON and payload values are still dynamically represented. The short field names are an experiment, not a stable network standard. A future binary encoding can reuse the same transition dictionary without changing conversation semantics.

## Measured result

On the checked-in Apple M4 release run, compact encoding reduced the checked-transition median from 2.417 µs to 1.833 µs, a 1.32× speedup. The complete warm exchange improved from 92.5 µs to 83.6 µs, a 1.11× speedup. Relative to the hand-written baseline, median overhead fell from 1.68× to 1.51×, still above the provisional 1.25× target.

This shows that specialization helps, while also locating the next costs in state-machine work, allocation, thread creation, and channels. See [the benchmark](benchmark.md) for methodology and limitations.
