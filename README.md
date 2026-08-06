<div align="center">

# Eve

**A server-native language for evolving intelligence.**

**The graph is the program.** Compile computation, communication, placement, and evolution into one executable plan.

[RFC-0001](rfcs/0001-eve-language-kernel.md) · [Vision](docs/vision.md) · [Design](docs/design.md) · [Architecture](docs/architecture.md) · [Text projection](docs/language.md) · [Roadmap](docs/roadmap.md)

</div>

---

> [!IMPORTANT]
> Eve is in the design phase. The examples in this repository are design sketches, not a stable language specification or working compiler yet.

AI software is becoming distributed, persistent, and increasingly authored by other software. Its unit of execution is no longer a process on one machine: it is a changing graph of models, tools, memory, accelerators, and services spread across a data center.

Eve explores what a language would look like if that reality were the starting point.

## The radical path

Eve is not fundamentally a collection of source files. Its source of truth is a **typed, content-addressed distributed graph**. Text, visual tools, and conversations are lossless projections and editors of that graph. AI systems operate through typed graph queries and transactions rather than being forced to regenerate files.

```text
text · visual · AI structural edits
              ↕
       canonical Eve Graph
          ↙     ↓      ↘
      local   cluster   evolved candidates
```

Meaning remains stable and inspectable. Compilers and evolutionary systems may synthesize implementations, placements, encodings, and wire protocols behind those contracts. Read the decision in [RFC-0001: The Eve language kernel](rfcs/0001-eve-language-kernel.md).

The following is one possible text projection—not the authoritative representation:

```eve
network training {
    node learner: gpu[8]
    node archive: storage

    stream experience: Batch<f16> from archive to learner
        latency < 2ms
        delivery at_least_once

    population policy: Model<Policy> on learner

    evolve policy every 10_000 steps {
        propose variants: 16
        evaluate on suite("safety-and-reward")
        promote best when safety >= parent.safety
    }
}
```

The compiler would turn this into a typed distributed program, choose transports such as shared memory, QUIC, or RDMA, place work on available hardware, and enforce the policy around model evolution.

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

1. **The network is part of the program.** Local and remote operations must not look accidentally identical; their cost and failure semantics stay visible.
2. **Intent is separate from mechanism.** Programs state constraints. The compiler and runtime choose a transport and execution plan.
3. **AI-native means structural and inspectable.** Eve exposes typed graph queries, transactions, holes, canonical projections, structured diagnostics, and stable semantics.
4. **Evolution is governed.** Generated variants run inside explicit capabilities, budgets, tests, and promotion rules. Self-modification is never implicit.
5. **Copying is a decision.** Ownership, tensor layout, locality, and data movement are represented so zero-copy paths can be used safely.
6. **Failure is typed.** Timeouts, partial delivery, node loss, and retries belong in function and stream contracts.
7. **Portable semantics, specialized execution.** The language stays vendor-neutral while backends exploit specific accelerators, NICs, and fabrics.

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
spec/
  eve-graph-...json  Experimental machine-readable graph schema
examples/
  hello.eve          Minimal server-to-server flow
  hello.evegraph.json  The same idea as a typed incomplete graph
  evolution.eve      Bounded evolutionary loop
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
