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
- one immutable projected endpoint graph per role, including typed failure edges;
- a deterministic compact-wire transition dictionary shared by both roles.

The plan identity covers the plan format, conversation identity, complete projected endpoint graphs, and compact transition dictionary. It detects accidental modification when the expected identity is retained or pinned; it is not a signature, provenance proof, or authorization mechanism.

Deserialized plans are verified once for version, digest, unique roles and states, valid state targets, known peer roles, endpoint format, and conversation agreement. The verifier does not reconstruct the original global conversation and does not yet prove endpoint duality independently of compilation.

For compatibility with plans produced before the dictionary existed, `wire` is optional in the v0 JSON representation. Preparing such a plan deterministically derives the dictionary from the verified endpoints. Newly compiled plans always include it.

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

Select the plan-backed compact encoding without changing the conversation:

```bash
cargo run -- run-plan build/generate.eveplan.json \
  --wire compact --transport quic
```

The runtime stores each endpoint graph and the compact dictionary behind shared `Arc` references. Starting an endpoint session clones its graph reference plus the two identities and initializes only local state and sequence; starting a compact transport clones the dictionary reference. Neither path clones compiled content. Every execution report contains both `conversation_identity` and `plan_identity`.

The distinction matters:

- the conversation identity names portable meaning;
- the plan identity names one projected representation of that meaning;
- a future specialized plan may change encoding, placement, batching, or transport while preserving the conversation identity.

## Compact specialization

The dictionary deduplicates matching endpoint actions, sorts them deterministically by state and semantic operation, and assigns dense `u16` IDs starting at one. The sender maps an already checked semantic frame to an ID. The receiver resolves that ID, reconstructs the full semantic frame, and runs the unchanged endpoint checks. Reference and compact sessions must produce identical semantic traces.

Compact mode currently requires both endpoints to possess the same plan before communication. Plan negotiation and a session handshake are not yet implemented. See [Eve Wire v0](wire.md) for the exact envelope and trust boundary.

## Measured boundary

On the compact-wire Apple M4 release run, compiling the example took a 51.6 µs median and starting both plan-backed sessions took 125 ns. Compact encoding reduced an isolated checked transition from 2.417 µs to 1.833 µs. Warm whole-exchange execution improved from 92.5 µs with reference envelopes to 83.6 µs with compact envelopes.

Compact warm execution was still 1.51× the 55.2 µs conventional baseline, missing the provisional 1.25× target. The next split should isolate state-machine checking, lookup, allocation, channel transfer, and thread scheduling. See [the benchmark](benchmark.md) for methodology and limitations.
