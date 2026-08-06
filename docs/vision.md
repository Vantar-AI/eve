# Vision

## The premise

Near-future AI systems will not be single models behind request-response APIs. They will be populations of specialized components that train, serve, evaluate, replicate, and change across many machines. Some changes will be authored by people; increasingly, others will be proposed by models or evolutionary search.

The dominant programming abstractions were not designed for this:

- General-purpose languages understand processes and memory, but not model placement or collective communication.
- RPC systems understand calls between known services, but not changing computational graphs or accelerator-local data.
- Orchestrators understand containers and desired replicas, but not the semantics of tensors, streams, or evaluation gates.
- AI frameworks understand graphs and gradients, but usually treat the surrounding distributed system as configuration and runtime convention.

Eve's thesis is that computation, communication, placement, and governed evolution should share one semantic model.

## The target

Eve targets persistent AI workloads running inside data centers and machine clusters:

- distributed training and inference;
- mixtures of experts and model routing;
- synthetic-data and evaluation pipelines;
- populations used by evolutionary algorithms;
- model-serving systems that adapt placement to demand;
- robotics or embodied fleets coordinating with server-side intelligence;
- heterogeneous compute spanning CPUs, GPUs, novel accelerators, and eventually biological or neuromorphic substrates.

Eve is not primarily an edge-function language. It assumes long-running state, high-rate server-to-server communication, heterogeneous hardware, topology, partial failure, and workloads that can change while the system remains alive.

## What “AI-native” means

AI models can generate existing languages. A language does not become AI-native merely by resembling English.

For Eve, AI-native means:

- one canonical representation for every valid program;
- a compact and regular grammar with few context-dependent rules;
- an AST and typed IR available as first-class compiler APIs;
- machine-readable diagnostics with causal traces and suggested repairs;
- explicit capabilities and resource budgets for generated code;
- inexpensive validation, simulation, and differential testing;
- semantic versioning that lets an agent determine exactly which rules apply;
- provenance for generated changes and reproducible promotion decisions.

Human readability remains essential because people must audit the systems that models produce.

## The end state

An Eve program should be able to describe an AI system as a typed graph of state and behavior, compile it for the available data-center topology, and safely accept candidate revisions while it runs.

The language should make the following sentence precise:

> Run this population across these resources, move this information under these latency and privacy constraints, evaluate new variants under this budget, and promote only changes that preserve these invariants.

## Relationship to Vantar AI

Vantar AI asks how software can span unconventional computational substrates. Nuro focuses on expressing and deploying computation across neuromorphic and related hardware. Eve explores the distributed layer: how intelligent workloads communicate, coordinate, and evolve across servers and substrates.

The projects can remain independent. A future Eve backend could invoke Nuro-compiled components without making Nuro part of Eve's core semantics.
