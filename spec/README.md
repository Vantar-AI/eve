# Eve Graph specification

This directory contains machine-readable experiments for the canonical Eve Graph described by [RFC-0001](../rfcs/0001-eve-language-kernel.md).

## Files

- [`eve-graph-v0.schema.json`](eve-graph-v0.schema.json) — JSON Schema for the first interchange experiment.

The JSON representation is an interchange and debugging format. It is not yet the canonical binary encoding and must not be treated as stable.

The current v0 schema predates the complete conversation-state model in [RFC-0002](../rfcs/0002-conversation-is-the-computation.md). Its cells and flows can represent topology, but choices, continuations, roles, and endpoint projection require the next schema revision.

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
