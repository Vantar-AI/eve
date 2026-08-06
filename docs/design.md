# Design

This document records the initial semantic direction. It is a hypothesis to test, not a frozen specification.

## One language, three representations

Eve should have three deliberately different representations:

1. **Source language** — concise, regular, and readable by people and models.
2. **Eve IR** — canonical, typed, versioned, and independent of a particular runtime.
3. **Execution plan** — topology- and backend-specific instructions produced for a deployment.

Source syntax may change. IR compatibility and execution semantics require a much higher stability bar.

## Core semantic objects

### Node

A logical placement target with declared capabilities. A node might resolve to a process, server, accelerator group, storage system, or isolated sandbox.

### Cell

A stateful unit of behavior. Cells receive typed messages, own state, and execute concurrently. They are location-transparent only when their contract permits migration.

### Stream

A typed, directed flow between cells or nodes. A stream declares delivery, ordering, backpressure, latency, privacy, and durability requirements. Local and remote streams share APIs but preserve distinct effects and costs.

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

1. What source-level problem does it solve?
2. What exact IR semantics does it introduce?
3. How can a runtime implement it on existing infrastructure?
4. What benchmark or failure test proves its value?
