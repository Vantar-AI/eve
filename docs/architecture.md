# Architecture

Eve is proposed as a language, compiler, runtime, and wire contract. Those components should be separable so the project can reuse proven infrastructure.

```text
Text · visual editor · AI graph transactions
    │ parse/project, resolve, type/effect/capability check
    ▼
Canonical semantic Eve Conversation Graph
    ├── History graph: patches, evidence, lineage, promotion
    │
    │ endpoint-project, partition, place, schedule, specialize
    ▼
Signed execution plan
    ├── endpoint machines: one local protocol state machine per role
    ├── compute backends: native, accelerator DSLs, Nuro, framework interop
    ├── transports: shared memory, QUIC, TCP, RDMA, collective libraries
    ├── storage: local, object, distributed log
    └── control plane: discovery, identity, policy, rollout
```

## Compiler

The compiler and graph store should perform progressively lower transformations:

1. Accept a text import or typed graph transaction against a known content identity.
2. Resolve types, shapes, effects, capabilities, holes, and failure contracts.
3. Canonicalize and content-address the semantic Eve Graph.
4. Project the global conversation into compatible local endpoint machines.
5. Partition those machines into cells and communication transitions.
6. Accept a topology and capability inventory from the target environment.
7. Produce and cost candidate placement, encoding, and transport plans.
8. Emit signed artifacts and a reproducible plan manifest.

MLIR is a strong candidate for internal compiler infrastructure, but Eve's portable semantics should not be defined merely as whichever MLIR dialects happen to exist.

## Runtime

The runtime is responsible for:

- node discovery and authenticated membership;
- artifact loading and capability assignment;
- stream creation, flow control, and transport negotiation;
- conversation-state validation, choice propagation, delegation, and cancellation;
- clock and cancellation propagation;
- state checkpoints and deterministic replay where requested;
- metrics and causal tracing using stable program identities;
- controlled rollout, comparison, and rollback of variants.

The fast data plane should not route every message through a central coordinator. The control plane may establish policy and placement while cells communicate directly over the selected transport.

Each endpoint executes a projected local state machine. A frame that is type-correct but invalid in the current conversation state is rejected or handled by a declared failure transition.

## Wire contract

The semantic graph fixes message meaning while an execution plan chooses representation. The first Eve wire format should prioritize correctness and measurement over novelty:

- a small versioned envelope for identity, schema, deadlines, tracing, and capabilities;
- canonical schema hashes and compatibility rules;
- separate control and bulk-data paths;
- transport negotiation rather than one mandatory transport;
- zero-copy descriptors when both endpoints and the security policy permit them;
- no implicit code execution when decoding data.

Existing encodings and transports should be used until a benchmark demonstrates a material reason to replace them.

## Evolution service

Evolution is an optional runtime service, not a privileged escape hatch. It receives candidates, validates them, runs bounded evaluations, records provenance, and asks the policy engine whether promotion is permitted. See [evolution.md](evolution.md).

Candidates arrive as typed patches against an immutable parent graph. The active graph is never rewritten in place.

## Interoperability

Initial interoperability should be pragmatic:

- a C ABI for embedding cells;
- import/export of a stable schema representation;
- adapters for Python and Rust host applications;
- tensor exchange through established array interfaces where possible;
- compiler hooks for existing kernel and model compilers.

The prototype should prove that Eve adds semantic and performance value without requiring an entire ecosystem to be rewritten.

## Current prototype

The first Rust implementation covers the top of this pipeline:

```text
Conversation v0 JSON
    → representation and semantic validation
    → global conversation state graph
    → client endpoint machine + server endpoint machine
    → independent endpoint execution
       ├── memory plan
       ├── length-delimited TCP plan
       └── TLS-authenticated QUIC plan
    → equivalent semantic trace identities
```

Each endpoint checks the conversation name, experimental semantic hash, expected state, and sequence before accepting a frame. The memory, TCP, and QUIC plans serialize the same versioned Eve Wire envelope. The reference `Generate` workload completes normally or through the cancellation branch without transport-specific application logic.

This runtime remains an experiment rather than a deployment substrate. It is blocking, supports two static roles, and sends JSON payloads. QUIC encrypts the connection and authenticates the server through an explicitly pinned certificate, but persistent identities, client authentication, authorization, flow control, structural payload validation from Eve type definitions, deadline enforcement, recovery, and transport negotiation remain unimplemented.
