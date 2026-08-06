# RFC-0002: The conversation is the computation

- **Status:** Draft
- **Created:** 2026-08-06
- **Requires:** RFC-0001
- **Target:** Eve Conversation v0
- **Authors:** Vantar AI

## Summary

Eve is the language servers speak to compute together.

An Eve program is not divided first into independent server programs joined later by APIs. It begins as a typed global conversation among roles. The compiler projects that conversation into local endpoint machines, then specializes their wire representation for the target topology.

The central invariant is:

> A server may perform the next communication step only when that step is valid in the shared conversation contract.

Servers exchange data, choices, channels, capabilities, graph references, continuations, and bounded adaptation proposals. Communication is not an effect at the edge of computation. Communication advances the computation.

## Clarification of RFC-0001

RFC-0001 establishes the canonical Eve Graph. This RFC identifies the graph's primary executable meaning: a **conversation graph**.

- Cells are participants or local computations.
- Ports are role boundaries.
- Flows are ordered conversation steps, not merely data pipes.
- The semantic graph contains a global protocol.
- Endpoint projection derives each participant's legal local state machine.
- Eve Wire carries the runtime transitions of those machines.

The execution plan may aggressively optimize or fuse those transitions, but it must preserve the observable conversation contract.

## Motivation

Conventional distributed software is authored as separate programs:

```text
client code + server code + schema + SDK + deployment + retry policy
```

The intended conversation exists only implicitly across those artifacts. Each participant implements its own partial understanding. Mismatched ordering, retries, version assumptions, cancellation, and failure behavior appear at runtime.

Eve authors one shared interaction:

```text
global conversation
    → endpoint program for role A
    → endpoint program for role B
    → endpoint program for role C
    → specialized wire plan
```

The compiler owns the mechanical duplication. People and AI systems work on the protocol as a whole.

## Foundations

The idea has deep precedents.

- The π-calculus models changing communicating processes and can transmit communication links or processes as values.
- Session types describe the legal sequence of messages in a communication session.
- Multiparty session types describe a global protocol and project local behavior for each role.
- Choreographic programming writes the distributed interaction once and compiles endpoint implementations.
- Actor and state-machine languages model isolated participants exchanging asynchronous messages.

Eve does not claim to invent communicating processes. Its proposed step is to make them the substrate for AI-authored server systems, content-addressed graph transactions, synthesized high-performance transports, and governed protocol evolution.

## Decision

### Conversations are first-class values

A conversation has:

- a semantic `ContentId`;
- named roles whose names are metadata over structural role identities;
- typed states and transitions;
- capability requirements;
- time, failure, ordering, and durability rules;
- entry and terminal states;
- adaptation boundaries;
- a compatibility relation;
- one or more projected endpoint implementations.

A conversation may be stored, passed, instantiated, composed, delegated, suspended, resumed, compared, and proposed as a candidate.

### Global meaning, local execution

The global conversation is authoritative. No server executes the global graph directly. Each server executes a projected endpoint machine containing only:

- its local state;
- its permitted sends and receives;
- choices it may make or must learn;
- capabilities it may exercise or delegate;
- continuations reachable from its role;
- evidence required before accepting an adaptation.

### Messages carry continuation identity

Every conversation frame identifies the expected protocol state. A valid frame advances the endpoint to its next continuation. A stale, duplicated, out-of-order, or incompatible frame is handled according to the contract rather than guessed from application code.

### Protocol and encoding are separate

The conversation graph specifies meaning. The execution plan specifies encoding, batching, memory layout, transport, and offload.

Two executions may use different wire plans while implementing the same conversation identity.

## Core calculus

The semantic kernel can be sketched as:

```text
P ::=
    end
  | A -> B : T ; P
  | choice A { label_i : P_i }
  | parallel(P, P)
  | spawn A as B with G ; P
  | delegate A.role to B ; P
  | call Conversation<roles> ; P
  | adapt boundary using Proposal ; P
  | fail F
```

Where:

- `A -> B : T ; P` transfers a value of type `T` and continues as `P`.
- `choice A` makes `A` responsible for selecting a labeled continuation and informing roles whose behavior depends on it.
- `parallel` composes independent conversations whose effects and roles permit concurrency.
- `spawn` creates a new role from content-addressed code under explicit placement and capability constraints.
- `delegate` transfers a role endpoint and its attenuated capabilities.
- `call` composes a reusable conversation.
- `adapt` proposes a replacement continuation inside a declared boundary.
- `fail` terminates or transfers control according to a declared failure branch.

This calculus is a semantic sketch, not final surface syntax.

## Text projection example

```eve
conversation Generate(prompt: Prompt) -> stream<Token> {
    roles client, router, expert[*]

    client -> router: prompt within 2ms

    choice router {
        cached {
            router -> client: CachedResult
            end
        }

        infer(expert) {
            router -> expert: prompt

            repeat token_loop {
                expert -> client: Token

                choice client {
                    continue { continue token_loop }
                    stop     { client -> expert: Cancel; end }
                }
            }
        }
    }
}
```

The compiler derives separate endpoint machines for `client`, `router`, and each `expert`. It also derives which participants must learn each choice and which messages may be concurrent.

## Endpoint projection

Endpoint projection maps a global conversation to one local machine per role.

For each role, projection must derive:

- local send and receive transitions;
- local choice or branch states;
- knowledge required to distinguish branches;
- channel and capability ownership;
- cancellation and timeout behavior;
- recovery states;
- compatible upgrade points.

Projection fails when a global graph requires a participant to behave differently across branches it cannot distinguish, uses a capability it does not possess, or relies on communication ordering the contract does not establish.

The v0 checker should provide a counterexample trace and the smallest ambiguous subgraph when projection fails.

## Eve Wire

Eve Wire is the machine-facing language spoken by projected endpoints.

### Control transitions

The initial control vocabulary is:

```text
OPEN      instantiate a conversation and bind roles
DATA      transfer a typed value or bulk-data descriptor
SELECT    announce a labeled branch
DELEGATE  transfer a role, channel, or attenuated capability
SPAWN     request a new participant from content-addressed code
PROPOSE   offer an alternative plan or continuation
ACCEPT    accept a compatible proposal
REJECT    reject a proposal with structured reasons
CANCEL    propagate cancellation and its scope
CLOSE     complete a conversation branch
FAULT     report a declared failure transition
```

These are semantic operations, not fixed byte opcodes yet.

### Frame envelope

Every control frame carries or derives:

```text
conversation_content_id
wire_plan_id
session_id
sender_role
receiver_role or group
current_state_id
transition_id
message_id
deadline or cancellation scope
capability references
trace context
payload descriptor
```

The envelope may be compiled away, compressed, or established once per channel on a trusted fast path. The runtime must still be able to reconstruct its semantic identity for tracing and conformance.

### Bulk data

Large tensors and streams do not pass through a verbose generic message encoder by default.

`DATA` may reference:

- inline bytes;
- shared memory;
- registered RDMA memory;
- accelerator memory;
- an object or content-addressed chunk;
- a collective operation;
- a generated encoding negotiated for this conversation.

Ownership, lifetime, layout, synchronization, and failure remain part of the typed transition.

## Mobility

Eve conversations may transmit more than ordinary values.

### Channel mobility

A role may delegate a channel endpoint, allowing the communication topology to change during execution. Delegation transfers the authority to use that endpoint; copying a reference does not duplicate linear authority.

### Code mobility

A conversation may reference content-addressed code and request that a compatible runtime instantiate it. The receiver validates signature, required capabilities, resource budget, target compatibility, and policy before execution.

The wire does not accept arbitrary executable bytes as trusted merely because another participant sent them.

### Continuation mobility

A suspended continuation may move with its serializable state and capability references when its contract permits migration. Non-migratable resources must be closed, delegated, or represented by location-bound handles.

## Negotiation and adaptation

Participants may negotiate execution without renegotiating meaning.

Examples include:

- selecting QUIC, shared memory, RDMA, or a collective;
- selecting tensor layout or quantization accepted by the contract;
- changing batch size or replica placement;
- moving a role closer to data;
- replacing a continuation with a compatible optimized implementation;
- selecting among model versions satisfying the same port contract.

A `PROPOSE` operation contains:

- parent conversation and state identity;
- proposed continuation or plan identity;
- claimed compatibility relation;
- effect, failure, capability, and resource delta;
- conformance evidence;
- validity window and rollback information.

Acceptance is local only when policy grants that role the authority to decide. Security or production-policy changes require the appropriate external signer.

## AI-authored protocols

AI systems interact with conversations structurally.

They may:

- fill a typed conversation hole;
- propose a branch, role, or continuation;
- synthesize compatible endpoint logic;
- search wire plans under latency and bandwidth objectives;
- infer missing failure branches from counterexample traces;
- propose a protocol mutation for isolated evaluation;
- compress or specialize a frequent conversation.

The checker derives the actual effects, authority, and compatibility. It does not trust the proposal's description of itself.

Opaque emergent communication is permitted only behind a stable typed transition with an independent encoder/decoder conformance oracle. Observability and policy frames remain intelligible.

## Failure semantics

Failure is a branch in the conversation, not an exception hidden inside one endpoint.

The global contract defines which roles learn about:

- timeout;
- cancellation;
- participant loss;
- rejected adaptation;
- capacity exhaustion;
- incompatible endpoint or plan;
- partial bulk transfer;
- revoked capability.

If the system cannot guarantee delivery of failure knowledge during a partition, the contract must include an uncertainty or timeout state rather than pretending all roles agree.

## Security

Conversation state is an authorization boundary.

- A frame valid in one state may be invalid in another even with the same payload type.
- A role can send only transitions permitted by its projected machine.
- Capabilities are explicitly granted, delegated, attenuated, and revoked.
- Code and continuation mobility require content verification and policy approval.
- A participant cannot unilaterally rewrite the shared conversation.
- Replayed frames are checked against message identity and state.
- The generic negotiation path cannot bypass a specialized fast path's security contract.

## Why this can outperform RPC

The benefit is not the cost of spelling `send` differently.

Because Eve sees the complete conversation, it can:

- eliminate separately maintained client and server protocol logic;
- check message order and role knowledge before deployment;
- fuse sequences of interactions into a specialized fast path;
- place computation using future communication dependencies;
- select zero-copy or collective paths from semantic tensor information;
- propagate cancellation and deadlines through the entire interaction;
- preserve protocol identity through generated implementations;
- evolve one continuation while proving or testing boundary compatibility;
- generate observability from the same states used for execution.

The performance claim must be benchmarked. A conversation abstraction that adds an indirection to every message without enabling specialization has failed.

## Minimal v0

Eve Conversation v0 supports:

- two or three statically known roles;
- typed `send`, `choice`, `repeat`, `cancel`, and `end`;
- explicit timeout and unavailable branches;
- endpoint projection into deterministic local state machines;
- a structured frame trace;
- one local transport and one remote transport;
- data payloads plus out-of-band bulk descriptors;
- protocol identity and state validation;
- no runtime code mobility;
- no automatic adaptation beyond transport selection.

Mobility and protocol evolution remain in the semantics so v0 choices do not make them impossible, but implementation follows evidence.

### Prototype status

The initial Rust prototype implements the two-role subset with typed transitions, choices, cyclic continuations, cancellation, terminal states, endpoint projection, and offline frame-trace validation. Projected endpoints now execute the same request/token/cancel graph through an in-process memory plan or a length-delimited TCP plan, including as separate client and server processes. Every envelope binds the experimental semantic conversation hash, state, and sequence. This is a correctness reference runtime, not yet an authenticated, encrypted, asynchronous, or optimized network runtime.

## Acceptance criteria

RFC-0002 may advance from Draft to Experimental when:

1. One global request/stream/cancel conversation projects into compatible endpoint machines.
2. Removing or reordering a required transition fails before execution with a counterexample trace.
3. A local and two-process execution produce equivalent semantic frame traces.
4. A participant cannot send a validly typed message in an invalid conversation state.
5. Cancellation reaches every role promised by the contract, or the trace records uncertainty explicitly.
6. The same conversation runs over two wire plans without changing semantic identity.
7. One specialized plan matches a hand-written baseline within a declared performance margin.
8. A proposed endpoint change that expands capabilities or failures is rejected.

## Alternatives considered

### Typed RPC only

Typed RPC checks individual calls but usually does not define the complete multi-message, multiparty conversation or participant knowledge of choices.

### Independent actor programs

Actors provide isolation and messaging, but global protocol consistency must be reconstructed from independently authored handlers unless an additional choreography or session layer exists.

### Natural language between servers

Natural language is valuable for uncertain semantic tasks. It is inefficient and ambiguous for transport contracts, authority, deadlines, ownership, and reproducible protocol state. Eve may carry natural-language values without using natural language as its execution semantics.

### Fully opaque emergent protocols

Agents may discover efficient codes, but an opaque system cannot reliably support independent implementations, governance, incident analysis, compatibility, or security. Eve allows emergent encodings behind explicit conformance boundaries.

## Prior art

- [Milner, Parrow, and Walker's π-calculus](https://doi.org/10.1016/0890-5401(92)90008-4) models mobile communicating processes whose links can change through communication.
- [Session types](https://www.cambridge.org/core/books/session-types/8B2999D0468C5088A59603E8030E467A) specify and verify communication behavior using types.
- [HasChor](https://arxiv.org/abs/2303.00924) expresses a global choreography and projects local endpoint programs.
- [Choral](https://www.choral-lang.org/) demonstrates higher-order choreographic programming and realistic full-duplex protocols.
- [P](https://p-org.github.io/P/advanced/psemantics/) models asynchronous event-driven distributed state machines and systematic protocol testing.

## Open questions

- Is the core better founded directly on a session-typed process calculus, an event-structure semantics, or both at different levels?
- Which asynchronous choreographies can be projected without injecting coordination messages?
- How should role discovery work when the number and identity of accelerator workers are dynamic?
- What is the compatibility relation for a live continuation upgrade?
- When may the planner fuse several conversation states into one transport operation?
- Can a generated encoding be validated cheaply enough for online specialization?
- How should causal traces survive fusion and hardware offload?
- Which mobile continuations can be safely serialized across heterogeneous runtimes?
