# Roadmap

Eve should advance by falsifiable prototypes, not by designing a large language in isolation.

## Phase 0 — Problem corpus

**Goal:** identify the irreducible language problem.

- Collect 10–20 real distributed AI workloads.
- Record their compute graph, communication, placement constraints, failure behavior, and operational incidents.
- Implement three representative workloads conventionally in Rust or Python with established RPC and collective libraries.
- Define measurements: end-to-end latency, tail latency, throughput, copies, bytes transferred, accelerator utilization, recovery time, and engineering complexity.

**Exit criterion:** at least two workloads share a semantic problem that existing libraries do not express cleanly.

## Phase 1 — Executable topology IR

**Goal:** validate semantics before inventing source syntax.

- Define a versioned graph schema for cells, flows, effects, capabilities, typed holes, and placement constraints.
- Define global conversations, roles, choices, continuations, and explicit failure branches.
- Build a validator and canonical serializer.
- Implement content identity, graph queries, and transactional structural patches.
- Implement endpoint projection into one deterministic local protocol machine per role.
- Prove through fixtures that renames, formatting, and projection order do not change semantic identity.
- Produce execution plans for local processes and a small multi-server testbed.
- Use existing transports; begin with shared memory and QUIC or TCP.
- Add deterministic fault injection and causal traces.

**Exit criterion:** one conversation graph projects into compatible endpoints and runs unchanged on a laptop simulation and a multi-node deployment, with explainable plan differences; a typed hole can be queried and filled without text rewriting.

**Current evidence:** the request/token/cancel graph now compiles once into a verified, identified Eve Plan whose shared endpoint graphs create lightweight sessions over memory, TCP, and authenticated QUIC. The CLI also runs TCP or QUIC roles as separate processes, and both report the same semantic conversation and trace identities. Deterministic faults preserve asymmetric timeout and uncertainty observations. A split conventional baseline shows plan reuse helps while full JSON envelopes remain expensive. Multi-node execution, canonical identity, typed structural patches, specialized wire plans, recovery semantics, and representative AI benchmarks remain open.

## Phase 2 — Minimal Eve front end

**Goal:** test whether a purpose-built language materially improves authorship and static checking.

- Implement the smallest text projection, parser/importer, formatter, type/effect checker, and structured diagnostics.
- Support cells, streams, schemas, placement constraints, deadlines, and capabilities.
- Support conversational send, choice, repeat, cancellation, and terminal states.
- Provide a Rust or Python embedding API.
- Compare model-generated graph transactions with text edits and equivalent changes in conventional infrastructure code.

**Exit criterion:** Eve prevents meaningful distributed failures before deployment and reduces the amount of workload-specific orchestration code.

## Phase 3 — Tensor and accelerator paths

**Goal:** demonstrate performance relevance.

- Add tensor shape, layout, ownership, and sharding information.
- Integrate one accelerator runtime and one collective or RDMA path.
- Compile communication/computation overlap where dependencies permit.
- Benchmark a mixture-of-experts routing or distributed inference workload.

**Exit criterion:** match the conventional implementation's median performance and improve either tail latency, utilization, portability, or correctness measurably.

## Phase 4 — Governed evolution

**Goal:** safely evaluate machine-authored program variants.

- Expose the typed AST and IR patch API.
- Build immutable lineage and artifact signing.
- Add bounded evaluation regions and invariant gates.
- Support canary promotion, compatible state migration, and rollback.
- Run an evolutionary search over a limited, measurable surface such as routing or placement.

**Exit criterion:** the system discovers and safely deploys an improvement while an injected invalid or authority-expanding candidate is rejected.

## First benchmark candidates

1. Dynamic batching across replicated inference servers.
2. Mixture-of-experts token routing under skewed load.
3. Distributed evaluation of a population of model variants.
4. Model pipeline spanning a conventional GPU and a Nuro-supported substrate.

## Decisions intentionally deferred

- Native code generation versus an MLIR-only backend.
- Garbage collection versus ownership-only memory management.
- A custom wire encoding.
- A custom transport.
- Precise surface syntax beyond what the prototype requires.
- Package registry and ecosystem governance.
