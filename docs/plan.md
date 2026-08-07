# Eve Plan v0

Eve Plan v0 moves validation and endpoint projection out of the per-session execution path.

A conversation remains the semantic source of truth. `eve compile` validates that global graph, calculates its experimental semantic identity, projects one endpoint machine per role, sorts those endpoints deterministically, and writes a reusable plan artifact:

```bash
cargo run -- compile examples/generate.eveconv.json \
  --out build/generate.eveplan.json
```

The JSON representation is a debugging and interchange form governed by [`eve-plan-v0.schema.json`](../spec/eve-plan-v0.schema.json). It is not an optimized wire encoding.

## Plan contents

An Eve Plan contains:

- the Eve Plan format version;
- the conversation name and experimental semantic identity;
- a deterministic plan identity;
- one immutable projected endpoint graph per role, including typed failure edges.

The plan identity covers the plan format, conversation identity, and complete projected endpoint graphs. It detects accidental modification when the expected identity is retained or pinned; it is not a signature, provenance proof, or authorization mechanism.

Deserialized plans are verified once for version, digest, unique roles and states, valid state targets, known peer roles, endpoint format, and conversation agreement. The verifier does not reconstruct the original global conversation and does not yet prove endpoint duality independently of compilation.

## Session execution

Run a compiled artifact without revalidating or re-projecting the conversation:

```bash
cargo run -- run-plan build/generate.eveplan.json \
  --transport memory --tokens 3
```

TCP and QUIC use the same plan:

```bash
cargo run -- run-plan build/generate.eveplan.json --transport tcp
cargo run -- run-plan build/generate.eveplan.json --transport quic
```

The runtime stores each endpoint graph behind an `Arc`. Starting a session clones the shared reference plus the two identities and initializes only local state and sequence. It does not clone the state graph. Every execution report now contains both `conversation_identity` and `plan_identity`.

The distinction matters:

- the conversation identity names portable meaning;
- the plan identity names one projected representation of that meaning;
- a future specialized plan may change encoding, placement, batching, or transport while preserving the conversation identity.

## Measured boundary

On the initial Apple M4 release run, compiling the example took a 40.0 µs median and starting both plan-backed sessions took 125 ns. Reusing the plan reduced whole-exchange median latency from 114.5 µs cold to 74.1 µs warm, a 1.55× speedup.

Warm execution was still 1.77× the conventional baseline, missing the provisional 1.25× target. The isolated checked transition was 2.21 µs versus 0.25 µs for baseline JSON, showing that full-envelope encoding—not endpoint session construction—is now the dominant local overhead. See [the benchmark](benchmark.md) for methodology and limitations.

## Next plan experiment

Eve Plan v1 should specialize the wire representation while retaining reconstructable semantic evidence:

1. Establish conversation, plan, role, and state identities once per session.
2. Replace repeated strings with compact transition IDs.
3. Precompute legal transition tables and payload codecs.
4. Measure protocol checking separately from encoding and channel transfer.
5. Require the optimized path to produce the same semantic trace as the reference plan.
