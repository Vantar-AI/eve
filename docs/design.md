# Design

This document records the initial semantic direction. It is a hypothesis to test, not a frozen specification.

## The graph is the program

Eve's authoritative representation is a canonical typed graph. It is not reconstructed from source files every time a tool needs semantic information.

The graph separates three artifacts:

1. **Semantic graph** — portable program meaning: types, cells, flows, effects, authority, constraints, and holes.
2. **Execution plan** — topology- and backend-specific realization of that meaning.
3. **History graph** — candidate changes, evidence, approvals, rollout, and lineage.

Text, visual canvases, and AI operations are projections and editors. They must preserve the semantic identity of unchanged graph content. See [RFC-0001](../rfcs/0001-eve-language-kernel.md).

## Identity

Names are mutable metadata. Immutable definitions and semantic modules are identified by hashes of canonical content. Runtime instances and governed release slots have separate identities.

This means a rename does not rebuild dependents, while a behavioral change always creates new content with traceable lineage.

## Typed holes

Incomplete programs have meaning. A typed hole records the interface it must satisfy, bindings in scope, allowed effects and failures, capability ceiling, resource budget, and outstanding obligations.

AI systems should normally receive a relevant graph slice plus one or more holes. Filling a hole is a typed graph transaction; it cannot silently expand authority or weaken the enclosing contract.

## Core semantic objects

### Conversation

A global, typed interaction among roles. The conversation specifies legal message sequences, choices, continuations, time, failure, authority, and adaptation boundaries. The compiler projects one local endpoint machine for every role. See [RFC-0002](../rfcs/0002-conversation-is-the-computation.md).

### Role

A participant in a conversation. A role is semantic; an execution plan binds it to one or more cells, processes, servers, accelerators, or external adapters.

### Node

A logical placement target with declared capabilities. A node might resolve to a process, server, accelerator group, storage system, or isolated sandbox.

### Cell

A stateful unit of behavior. Cells receive typed messages, own state, and execute concurrently. They are location-transparent only when their contract permits migration.

### Stream

A typed, directed sequence of conversation transitions between roles. A stream declares delivery, ordering, backpressure, latency, privacy, and durability requirements. Local and remote streams preserve distinct effects and costs even when they share a contract.

### Region

A set of resources and data governed by common placement, security, or failure constraints.

### Population

A set of versioned models or programs that can be evaluated and selected as a group. Populations are optional; ordinary distributed programs should not pay for evolutionary features.

### Policy

An executable constraint controlling capabilities, resource use, placement, data access, or promotion. Policy evaluation must be deterministic for a given input and policy version.

## Effects

Eve should make important distributed effects visible in types or signatures:

```eve
fn infer(input: Tensor<f16>) -> Prediction
    uses gpu
    sends telemetry
    may timeout, unavailable
```

The initial effect set should remain small:

- remote communication;
- durable state mutation;
- accelerator use;
- nondeterminism;
- capability use;
- declared failure modes.

## Ownership and data movement

Large AI messages are frequently tensors. Copying one is not equivalent to copying a small value.

The language needs to distinguish:

- owned, borrowed, shared, and immutable data;
- host, accelerator, and remotely registered memory;
- logical tensor shape from physical sharding and layout;
- a control message from a bulk data plane transfer;
- serialization from a transport-supported zero-copy view.

The source language should express intent. The execution plan records exact buffers, transfers, and synchronization.

Here, “source language” means any authoring projection. The canonical semantic graph remains authoritative.

## Failure

Remote execution introduces failure that a normal function call does not have. Eve must avoid hiding that fact.

Streams and calls declare applicable behavior: timeout, retry, deduplication, cancellation, reordering, and partial results. “Exactly once” should not be a magical keyword; it must lower to a specific protocol with explicit storage and failure assumptions.

## Determinism

Determinism is opt-in at the cell or region level. A deterministic region controls clocks, random seeds, scheduling inputs, external effects, and floating-point assumptions sufficiently to support replay. Other regions may choose throughput over replayability.

## Security

Authority is capability-based. A generated component receives only the handles and budgets required by its contract. Network identity alone does not grant access.

Code evolution never expands authority automatically. A candidate requiring a new capability must be approved by an external policy or operator.

## Non-goals for the first prototype

- A novel CPU or accelerator instruction set.
- A new general-purpose standard library.
- A custom network transport before existing transports are measured.
- Transparent distributed shared memory.
- Unbounded runtime self-modification.
- Compatibility with every AI framework.

## Test for every feature

A proposed feature should answer four questions:

1. What authoring or semantic problem does it solve?
2. What exact IR semantics does it introduce?
3. How can a runtime implement it on existing infrastructure?
4. What benchmark or failure test proves its value?
