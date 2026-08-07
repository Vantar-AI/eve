# Eve Graph specification

This directory contains machine-readable experiments for the canonical Eve Graph described by [RFC-0001](../rfcs/0001-eve-language-kernel.md).

## Files

- [`eve-graph-v0.schema.json`](eve-graph-v0.schema.json) — JSON Schema for the first interchange experiment.
- [`eve-conversation-v0.schema.json`](eve-conversation-v0.schema.json) — executable two-role conversation interchange used by the Rust prototype.
- [`eve-plan-v0.schema.json`](eve-plan-v0.schema.json) — compiled two-endpoint execution-plan artifact, including the optional compact transition dictionary used to start reusable sessions.

The JSON representation is an interchange and debugging format. It is not yet the canonical binary encoding and must not be treated as stable.

The original Eve Graph v0 schema predates the conversation-state model in [RFC-0002](../rfcs/0002-conversation-is-the-computation.md). The separate Conversation v0 experiment now represents two roles, sends, choices, loops, cancellation, declared failures, success terminals, and failure terminals without pretending the broader Graph schema is already stable. In v0, an `on_failure` edge must target a terminal `fail` state carrying the same declared failure ID; retry and recovery graphs are deferred.

Eve Plan v0 is a derived artifact, not another semantic source. It carries the experimental conversation identity, deterministic plan identity, projected endpoint graphs, and a compiler-derived compact-wire dictionary. JSON Schema checks its representation; the Rust plan verifier additionally recalculates the digest, validates state and role references, and rejects a noncanonical transition table before sessions are created. Older v0 artifacts may omit `wire`; the runtime derives it when preparing the plan.

## Canonicalization experiment

Eve Graph v0 intends to calculate semantic identity from:

1. The declared semantic format version.
2. Definitions after reference resolution and alpha-normalization.
3. Types, behavior, effects, failures, contracts, capabilities, budgets, placement requirements, policies, and reachable holes.
4. External semantic dependencies by content identity.

The following are excluded:

- object key order and insignificant representation details;
- friendly names and aliases;
- comments and documentation;
- source locations;
- UI positions and view state;
- authorship and timestamps;
- cached derived facts and execution measurements.

The initial implementation must publish canonicalization fixtures before any produced digest is called an Eve `ContentId`.

## Validation boundary

JSON Schema validates representation shape. The Eve checker must additionally validate:

- uniqueness and resolution of local references;
- port direction and type compatibility;
- flow contract consistency;
- type, effect, failure, and capability constraints;
- reachable-hole closure for deployment entrypoints;
- policy isolation and authority non-amplification;
- canonicalization and content identity.
