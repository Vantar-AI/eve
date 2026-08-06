# Governed evolution

Evolutionary algorithms are a central motivation for Eve, but “the program rewrites itself” is too imprecise and unsafe to serve as a language feature.

Eve models evolution as a controlled transaction:

```text
parent artifact
    → candidate proposal
    → parse and type/effect check
    → capability and budget check
    → isolated evaluation
    → policy decision
    → canary rollout
    → promote or roll back
```

## Invariants

Every evolutionary operation must preserve these properties:

1. **Immutable lineage** — parent, transform, generator, inputs, and compiler version are recorded.
2. **No authority inheritance by accident** — a candidate cannot request or acquire capabilities merely because its parent had them.
3. **Bounded evaluation** — time, compute, memory, network, and data access have explicit limits.
4. **Independent gates** — candidate-generated tests cannot be the only promotion criteria.
5. **Reversible rollout** — promotion includes a compatible checkpoint or explicit migration and rollback strategy.
6. **Stable comparison** — metrics name their dataset, environment, seed policy, and measurement version.
7. **Human-defined constitutional policy** — some invariants are outside the candidate's editable program region.

## What can evolve?

Evolution occurs at typed boundaries:

- model parameters;
- model graph transformations from an approved set;
- cell implementations preserving an interface and effects;
- placement hints and communication plans;
- complete subgraphs whose external contracts remain compatible.

Changing a public schema, acquiring a new capability, weakening a safety gate, or expanding a resource ceiling requires a separate authority.

## Program variation

Models should manipulate Eve through the typed AST/IR API where possible. Text generation remains useful, but canonical structural edits reduce syntax failures and make provenance precise.

A candidate proposal contains at least:

```text
proposal_id
parent_artifact_hash
transform_id or structural patch
generator identity and version
requested capabilities and budget
claimed interface and effect summary
```

The compiler independently derives the actual interface and effect summary. It never trusts the candidate's claim.

## Evaluation isolation

Candidates execute in a region with:

- fresh or explicitly cloned state;
- synthetic, redacted, or approved evaluation data;
- no production write capability;
- bounded outbound communication;
- deterministic seeds when the experiment requires comparison;
- complete causal tracing.

Simulation is necessary but insufficient. Promotion policy may require hardware-in-the-loop evaluation or shadow traffic before a canary receives live work.

## Open research questions

- How can state migrations be proved compatible before a live evolutionary rollout?
- Which effects can be compared statically between parent and child?
- How should nondeterministic evaluations express confidence and stopping criteria?
- Can execution plans evolve independently from program semantics without compromising reproducibility?
- How should a population share discoveries without collapsing diversity?
