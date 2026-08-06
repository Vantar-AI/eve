# RFC-0001: The Eve language kernel

- **Status:** Draft
- **Created:** 2026-08-06
- **Target:** Eve Graph v0
- **Authors:** Vantar AI

## Summary

Eve is graph-native, not text-native.

The authoritative program is a typed, content-addressed graph describing computation, communication, placement constraints, authority, failure, and evolution boundaries. Text files, visual canvases, conversational descriptions, and AI editing tools are projections and editors of that graph. No projection owns the program.

The central invariant is:

> Every valid projection denotes the same canonical Eve Graph, and every semantic change is a typed graph transaction.

Eve separates three artifacts:

1. **Semantic graph** — portable program meaning.
2. **Execution plan** — topology- and backend-specific realization.
3. **History graph** — proposals, evaluations, approvals, and lineage.

This RFC defines the kernel shared by all three.

[RFC-0002](0002-conversation-is-the-computation.md) defines the graph's primary executable meaning: a global server conversation projected into local endpoint machines.

## Motivation

Text is an effective interface for people, but a poor universal source of truth for systems increasingly authored by people, models, compilers, and evolutionary search.

Text-first tooling repeatedly reconstructs program structure, identity, dependencies, and intent from files. Distributed systems then split additional meaning across RPC schemas, deployment manifests, permissions, retry configuration, and observability conventions.

Eve starts from different assumptions:

- the program is distributed by default;
- its authors may be software;
- incomplete candidate programs are normal;
- names are interfaces, not identity;
- network cost and failure are semantic;
- many equivalent implementations may coexist before one is selected;
- behavioral variants require lineage and governed promotion.

The goal is not to hide distribution. The goal is to represent it precisely enough that it can be checked, transformed, measured, and evolved.

## Decision

### The graph is the program

An Eve program is stored as canonical graph data. Human-readable source is generated from and parsed into this graph losslessly.

Consequences:

- a repository may export `.eve` text, but text files are not the fundamental database;
- formatting and declaration order cannot change semantic identity;
- tools operate on stable typed nodes rather than byte offsets whenever possible;
- merges are graph transactions with semantic preconditions;
- the compiler can explain changes as operations on program meaning;
- AI systems can query and patch only the relevant subgraph.

### Programs may be meaningfully incomplete

Typed holes are first-class graph nodes. A hole records its expected type, available bindings, permitted effects, capability ceiling, and unresolved obligations.

Incomplete graphs can be queried, formatted, compared, simulated around unaffected regions, and offered to an AI for completion. A deployment entrypoint must be closed: no reachable hole may affect its behavior unless the runtime contract explicitly models that hole as unavailable functionality.

### Names are metadata

Human names do not define semantic identity. Definitions and modules have content identities. Friendly names, documentation, ownership, and search aliases are mutable metadata attached to those identities.

A rename does not rebuild dependents. A semantic edit creates new content.

### Distribution is typed

Remote communication, time, delivery, ordering, backpressure, authority, and failure are represented in the graph. A remote flow is never silently equivalent to a local function call.

### Evolution is a graph transaction

AI-generated and evolutionary changes are proposed as typed patches against a known parent graph. They do not mutate the active artifact in place.

## Graph layers

### Semantic graph

The semantic graph contains portable meaning:

- types and schemas;
- cells and their state transitions;
- ports and flows;
- effects and failure sets;
- capabilities and budgets;
- placement constraints;
- policies and invariants;
- typed holes;
- permitted evolution boundaries.

It does not contain hostnames, sockets, concrete buffer addresses, vendor-specific collective algorithms, or ephemeral runtime identities.

### Execution plan

The execution plan is compiled from a semantic graph plus a target inventory and optimization objectives. It contains:

- selected implementations;
- concrete placement and replica counts;
- transport and encoding choices;
- memory layouts and data movement;
- scheduling and batching decisions;
- generated adapters;
- runtime resource reservations;
- proofs, checks, or benchmark evidence required by policy.

Changing a plan without changing observable semantics produces a new plan identity but not a new semantic graph identity.

### History graph

The history graph records:

- parent and candidate content identities;
- structural patch operations;
- proposer and toolchain identity;
- declared objective;
- derived type, effect, and capability differences;
- evaluation inputs and measurements;
- policy decisions and signatures;
- rollout, promotion, and rollback events.

History is append-only. A current release is a signed reference into history, not a mutable label embedded in code.

## Identity model

Eve distinguishes four identities.

### Content identity

`ContentId` identifies immutable semantic content using a versioned cryptographic hash of its canonical encoding.

```text
ContentId = hash(
    domain_separator,
    canonicalization_version,
    semantic_content
)
```

Annotations such as friendly names, source positions, comments, authorship, and UI layout are excluded. Types, effects, contracts, dependencies, and executable behavior are included.

Immutable definitions form a Merkle DAG. A module or deployment topology may contain internal cycles; such a unit is canonicalized and hashed as one artifact, with references to external definitions by `ContentId`.

### Slot identity

`SlotId` is a governed reference that may point to different content over time—for example `production/router`. Updating a slot is an auditable history event subject to policy.

### Instance identity

`InstanceId` identifies one runtime incarnation of a cell. Restarts and replicas receive distinct instance identities even when they execute identical content.

### Message identity

`MessageId` identifies a logical message for tracing, deduplication, and delivery contracts. A retransmission preserves the logical identity while acquiring a new transport-level identity.

## Semantic objects

### Type

The kernel supports structural values and nominal resource types.

Initial value forms:

- scalar;
- record;
- variant;
- tuple;
- sequence;
- bytes;
- tensor with element type, shape constraints, and logical layout;
- result with an explicit failure set;
- content reference.

Resource types represent owned or borrowed state, capabilities, devices, streams, clocks, and durable handles. They cannot be serialized merely because their fields appear serializable.

### Cell

A cell is the unit of state, isolation, placement, and evolution.

A cell definition contains:

- state type and initializer;
- typed input and output ports;
- transition handlers;
- required capabilities;
- declared effects and failure set;
- placement and resource constraints;
- concurrency policy;
- checkpoint and migration contract;
- evolution boundary.

By default, a cell processes one state transition at a time. Parallelism must be expressed through stateless handlers, partitions, child cells, or an explicit concurrency contract.

A transition conceptually produces a new state plus requested effects. Exact atomicity depends on the declared durability contract; the runtime must not imply exactly-once effects when the necessary storage assumptions are absent.

### Port

A port is a typed boundary on a cell. It has a direction, message type, effect constraints, and optional protocol requirements. Port compatibility is semantic, not name-based.

### Flow

A flow connects output ports to input ports. Its contract may include:

- delivery: best effort, at most once, at least once, or a fully specified transactional protocol;
- ordering scope;
- capacity and backpressure;
- deadline and cancellation propagation;
- durability;
- privacy and locality constraints;
- tensor ownership and layout requirements;
- permitted encodings and transports;
- behavior under partition or destination loss.

The compiler must reject underspecified combinations whose behavior would otherwise be guessed.

### Capability

A capability is an unforgeable, attenuable authority to perform an effect on a resource. There is no ambient network, filesystem, credential, model, or deployment authority.

Child cells and generated variants receive an explicit subset of the parent's capabilities. A graph rewrite cannot increase authority without a separate signed authorization event.

### Region

A region groups graph elements under common policy: failure domain, data residency, determinism, trust boundary, budget, or lifecycle.

Regions are semantic constraints, not necessarily physical clusters.

### Policy

A policy is a deterministic predicate over graph facts, plan facts, history facts, or runtime measurements. Policies controlling authorization or promotion live outside the editable boundary of the candidate they judge.

### Hole

A hole is an explicit incomplete node with:

- expected input and output types;
- bindings visible at the hole;
- maximum effects and failures;
- maximum capabilities;
- resource budget;
- proof or test obligations;
- provenance of attempted fillings.

A filling is accepted only if its derived contract is a subtype of the hole's ceiling.

## Effects and failures

Eve separates returned values, declared failures, and effects.

- A returned `Result<Value, Failure>` is part of ordinary control flow.
- A declared failure records conditions such as timeout, unavailable, cancelled, rejected, or exhausted.
- An effect records authority-bearing interaction such as remote send, durable write, clock access, randomness, accelerator use, or external I/O.

The type checker derives a conservative effect and failure summary. A candidate may remove effects without new authority. Adding an effect requires the containing contract and capabilities to permit it.

## Time

Distributed time is not a single global scalar.

The kernel distinguishes:

- monotonic local duration;
- wall-clock observation as a capability-bearing effect;
- logical and causal ordering;
- deadlines propagated through a flow;
- event time carried in data;
- processing time observed by a runtime.

No correctness rule may rely on synchronized wall clocks unless its required clock bound is explicit in the plan contract.

## Graph transactions

All edits use transactions against a known base `ContentId`.

Primitive operations include:

```text
insert(node, into)
remove(node, if_unreferenced)
replace(old, new, preserving)
connect(output_port, input_port, contract)
disconnect(flow)
refine(hole, candidate)
restrict(capability_or_budget)
rewrite(region, rule, proof_or_test)
```

A transaction contains preconditions, such as the expected type or content identity of the edited region. Commit performs canonicalization, reference resolution, type/effect checking, policy checks, and identity calculation atomically.

Failed transactions return structured diagnostics and a minimal conflicting subgraph. They never leave partially parsed program text.

## Projections

### Text projection

Eve Source is a deterministic, human-readable projection. It supports comments and documentation as annotations, but canonical formatting has exactly one representation for semantic content.

Parsing source produces a graph transaction. Printing the resulting graph and parsing it again must preserve semantic identity.

### Structural projection

Editors and AI tools use a typed graph API. Queries can request a dependency slice, a cell contract, reachable capabilities, an effect path, or all obligations around a hole without loading unrelated source.

### Visual projection

The same graph may be rendered as topology, dataflow, state machine, capability, or evolution views. UI position is annotation and does not change program meaning.

### Binary projection

Canonical binary encoding is used for hashing, storage, signatures, and efficient exchange. Binary format version and semantic version are separate.

## Synthesized protocols

The semantic graph fixes meaning; the execution plan selects representation.

For a compatible pair of ports, a planner may synthesize or select:

- serialization schema;
- compression and quantization;
- batching;
- shared-memory or registered-memory layout;
- stream multiplexing;
- transport;
- retry and deduplication machinery;
- accelerator collectives or switch/NIC offload.

Both endpoints authenticate the semantic contract identity and plan identity before exchanging data. An optimized encoding is valid only if it passes the contract's conformance oracle or carries evidence accepted by policy.

An opaque emergent code may be used inside this boundary. It never replaces the stable port semantics, observability envelope, or capability checks.

## Optimization and evolution

Eve distinguishes two kinds of change.

### Semantics-preserving optimization

A rewrite claims observational equivalence under stated assumptions. Multiple equivalent forms may coexist in an equality graph. A planner extracts a form minimizing a cost function such as latency, memory, energy, or transfer volume.

Equivalence claims require a trusted rewrite, proof, exhaustive finite check, differential test obligation, or policy-approved validation method.

### Behavioral evolution

A mutation intentionally changes behavior toward an objective. It always receives a new semantic `ContentId` and enters the history graph as a candidate. It may be evaluated and promoted, but never mislabeled as an optimization.

The candidate cannot edit the policy that evaluates it, forge measurements, expand its capability ceiling, or overwrite lineage.

## Lowering

The proposed lowering pipeline is:

```text
Eve Graph
  → resolved and checked graph
  → partitioned cell/flow graph
  → candidate execution graphs
  → target-specific compute and communication IRs
  → signed execution plan
```

Eve may use MLIR dialects, native code, accelerator compilers, existing AI frameworks, or external services. These are lowering targets, not the definition of Eve semantics.

## Minimal v0

The first implementation deliberately supports a narrow subset:

- records, variants, scalars, bytes, and tensor descriptors;
- cells with typed ports and handler holes or external implementations;
- flows with delivery, ordering, capacity, and deadlines;
- capability requirements and resource budgets;
- placement constraints;
- typed holes;
- deterministic graph canonicalization;
- structured graph transactions;
- JSON interchange plus a future canonical binary encoding;
- a local runtime and one remote transport;
- plan inspection and deterministic fault injection.

Evolution v0 records candidate patches and evaluation results. Automatic production promotion is out of scope.

## Acceptance criteria

RFC-0001 may advance from Draft to Experimental when:

1. The graph schema represents request/response, streaming inference, and a bounded evolutionary candidate.
2. Text and graph projections round-trip to the same semantic identity.
3. A typed hole can be queried and filled through the structural API.
4. Rename and formatting changes preserve semantic identity.
5. Effect or capability expansion is detected from a patch.
6. One graph runs unchanged in a local simulation and a two-process plan.
7. Killing a destination produces the declared failure rather than an indefinite hang.
8. A synthesized or selected wire plan passes an independent conformance test.

## Alternatives considered

### Text as the authoritative source

This retains compatibility with existing version control and editors. Eve still exports canonical text for those tools, but making text authoritative would weaken structural identity, typed patching, partial-program semantics, and machine authorship.

### A library in an existing language

A library can prototype the runtime and should be used as a baseline. It cannot fully control names, effects, partial programs, canonical graph identity, or deployment semantics without effectively constructing a language inside the host.

### An entirely emergent agent language

Task-specific agents can discover efficient communication codes, but unconstrained emergent languages are difficult to interpret, version, secure, and compose. Eve confines emergent protocols behind typed, testable contracts.

### One universal wire protocol

No single encoding or transport is optimal across small control messages, tensors, local memory, WAN links, collectives, and future hardware. Eve standardizes negotiation and meaning while allowing specialized plans.

## Prior art

- [Unison](https://www.unison-lang.org/docs/the-big-idea/) demonstrates content-addressed definitions and names as metadata.
- [Hazel](https://hazel.org/) gives static and dynamic meaning to incomplete programs through typed holes and structural editing.
- [P](https://p-org.github.io/P/advanced/psemantics/) models distributed protocols as asynchronously communicating state machines.
- [MLIR](https://mlir.llvm.org/docs/LangRef/) demonstrates extensible multi-level IR with textual, in-memory, and serialized representations.
- [egg and equality saturation](https://egraphs-good.github.io/egg/egg/tutorials/_01_background/) demonstrate compact representation and cost-based extraction of equivalent programs.
- Actor languages, dataflow systems, capability machines, distributed tensor systems, and content-addressed stores provide additional foundations surveyed in [prior-art.md](../docs/prior-art.md).

Eve's proposed contribution is not any one mechanism. It is their combination around a distributed, AI-authored, evolution-aware semantic graph.

## Open questions

- What is the smallest semantic core that supports both pure computation and stateful cells?
- Should recursive definition groups be hashed as modules or through a cycle-aware canonical graph algorithm?
- Which graph annotations must survive source export without affecting identity?
- What observational equivalence is practical for distributed, timed programs?
- How are compatible state migrations represented and checked?
- Can typed holes safely remain in dormant or policy-disabled graph regions?
- Which constraints belong in types versus policies?
- How should semantic identities survive standard evolution without freezing mistakes forever?
