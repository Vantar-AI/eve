<div align="center">

# Eve

**The language servers speak to think together.**

**The graph is the program.** Compile computation, communication, placement, and evolution into one executable plan.

[RFC-0002: Conversation](rfcs/0002-conversation-is-the-computation.md) · [RFC-0001: Kernel](rfcs/0001-eve-language-kernel.md) · [Vision](docs/vision.md) · [Architecture](docs/architecture.md) · [Roadmap](docs/roadmap.md)

</div>

---

> [!IMPORTANT]
> Eve is an early research prototype. The repository now contains an experimental conversation checker and endpoint projector, but not a network runtime, stable language, or production system.

AI software is becoming distributed, persistent, and increasingly authored by other software. Its unit of execution is no longer a process on one machine: it is a changing graph of models, tools, memory, accelerators, and services spread across a data center.

Eve explores what a language would look like if that reality were the starting point.

## The radical path

Eve is not fundamentally a collection of source files. Its source of truth is a **typed, content-addressed conversation graph**. Text, visual tools, and AI operations are lossless projections and editors of that graph. AI systems operate through typed graph queries and transactions rather than being forced to regenerate files.

```text
text · visual · AI structural edits
              ↕
       canonical Eve Graph
          ↙     ↓      ↘
      local   cluster   evolved candidates
```

Meaning remains stable and inspectable. Compilers and evolutionary systems may synthesize implementations, placements, encodings, and wire protocols behind those contracts. Read the decision in [RFC-0001: The Eve language kernel](rfcs/0001-eve-language-kernel.md).

## The conversation is the computation

Eve does not begin with separate client and server programs joined by an API. It begins with one global, typed conversation. The compiler projects that conversation into a local state machine for each server, accelerator, or service. At runtime those endpoints speak Eve Wire: typed transitions that advance their shared computation.

Read the model in [RFC-0002: The conversation is the computation](rfcs/0002-conversation-is-the-computation.md).

The following is one possible text projection—not the authoritative representation:

```eve
conversation Generate(prompt: Prompt) -> stream<Token> {
    roles gateway, router, expert[*]

    gateway -> router: prompt within 2ms

    choice router {
        cached { router -> gateway: CachedResult; end }
        infer(expert) {
            router -> expert: prompt
            expert -> gateway: stream<Token>
        }
    }
}
```

The compiler would derive compatible endpoint programs, choose transports such as shared memory, QUIC, or RDMA, place work on available hardware, and preserve one traceable conversation identity across them.

## First executable experiment

The Rust prototype implements a deliberately small slice of [RFC-0002](rfcs/0002-conversation-is-the-computation.md): two roles, typed sends, choices, loops, cancellation, terminal states, endpoint projection, and conversation-trace validation.

```bash
# Validate one global server conversation
cargo run -- check examples/generate.eveconv.json

# Derive one local protocol machine for each server role
cargo run -- project examples/generate.eveconv.json

# Accept a valid request → token → cancel conversation
cargo run -- verify-trace \
  examples/generate.eveconv.json \
  examples/traces/generate-cancel.valid.json

# Run the test suite
cargo test
```

Projection produces `build/endpoints/client.endpoint.json` and `server.endpoint.json`. The client and server receive dual actions: every send becomes the peer's receive, every selection becomes the peer's branch, and cancellation is explicit on both sides.

The invalid trace in `examples/traces/generate-wrong-order.invalid.json` demonstrates the central property: a `token` message has the correct data type, but Eve rejects it when the server has not first selected the `token` conversation branch.

## Why Eve?

Current AI infrastructure is assembled from application languages, RPC schemas, orchestration systems, accelerator kernels, collective libraries, and deployment configuration. Each layer has a different model of state, failure, and communication.

Eve aims to make five concerns part of the same program:

- **Computation** — what each model or service does.
- **Communication** — the typed data that moves between nodes.
- **Placement** — where work and state are allowed to live.
- **Coordination** — timing, delivery, synchronization, and failure behavior.
- **Evolution** — how candidate programs and models are generated, evaluated, promoted, and rolled back.

This is closer to a language for the data center than a language for an individual serverless function.

## Design principles

1. **The conversation is the computation.** A global protocol projects into compatible endpoint programs for every participating server.
2. **Intent is separate from mechanism.** Programs state constraints. The compiler and runtime choose a transport and execution plan.
3. **AI-native means structural and inspectable.** Eve exposes typed graph queries, transactions, holes, canonical projections, structured diagnostics, and stable semantics.
4. **Evolution is governed.** Generated variants run inside explicit capabilities, budgets, tests, and promotion rules. Self-modification is never implicit.
5. **Copying is a decision.** Ownership, tensor layout, locality, and data movement are represented so zero-copy paths can be used safely.
6. **Failure is typed.** Timeouts, partial delivery, node loss, and retries belong in function and stream contracts.
7. **Portable semantics, specialized execution.** The language stays vendor-neutral while backends exploit specific accelerators, NICs, and fabrics.
8. **Protocols may adapt; meaning stays governed.** Servers can negotiate optimized continuations and wire plans only inside typed, inspectable boundaries.

## What Eve is not

- A conversational language for agents.
- A replacement for every kernel language or AI framework.
- A new packet transport solely for the sake of novelty.
- A promise that arbitrary self-modifying systems are safe.
- A thin deployment configuration format.

Eve may compile model computation through projects such as [Nuro](https://github.com/Vantar-AI/nuro), while owning the distributed program around that computation.

## Repository map

```text
docs/
  vision.md          Long-term thesis and use cases
  design.md          Principles, semantic model, and non-goals
  architecture.md    Proposed compiler and runtime layers
  language.md        Illustrative language surface
  evolution.md       Governed evolutionary execution
  prior-art.md       Existing systems and Eve's intended gap
  roadmap.md         Validation plan from research to prototype
rfcs/
  0001-...md         Graph-native language-kernel proposal
  0002-...md         Server conversation and Eve Wire proposal
spec/
  eve-graph-...json  Experimental machine-readable graph schema
  eve-conversation-...json  Executable global conversation schema
examples/
  hello.eve          Minimal server-to-server flow
  hello.evegraph.json  The same idea as a typed incomplete graph
  generate.eveconv.json  Executable request/stream/cancel conversation
  traces/            Valid and deliberately invalid Eve Wire traces
  evolution.eve      Bounded evolutionary loop
src/
  lib.rs             Checker, endpoint projection, trace validation
  main.rs            Experimental `eve` CLI
```

## Current questions

The project begins with questions, not predetermined syntax:

- Can placement and communication effects be expressive without becoming infrastructure configuration?
- Can graph-native editing outperform text generation for people and AI without sacrificing ordinary version control?
- Which guarantees belong in the language, the IR, or only in a particular runtime?
- Can an evolutionary system modify a live distributed program while preserving capabilities and invariants?
- What is the smallest useful prototype that beats a conventional Rust/Python plus RPC implementation?
- Should Eve begin as a standalone compiler or as a front end targeting MLIR and existing runtimes?

## Contributing

Eve is currently a research and language-design project. The most useful contributions are concrete workloads, failure cases, small syntax proposals with lowering semantics, and measurements from real clusters. See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Apache 2.0. See [LICENSE](LICENSE).

---

<div align="center">

**[Vantar AI](https://vantar.xyz)** · [GitHub](https://github.com/Vantar-AI)

</div>
