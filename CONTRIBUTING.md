# Contributing to Eve

Eve is currently a documentation-first research project. Contributions should make the thesis more precise, more testable, or easier to falsify.

## Useful contributions

- A real distributed AI workload and its constraints.
- An incident or failure mode current abstractions make difficult to prevent.
- A minimal syntax proposal paired with exact IR and runtime semantics.
- A comparison with prior work that changes or narrows Eve's direction.
- A benchmark design with a conventional baseline.
- A security analysis of generated or evolutionary programs.

## Proposal format

Open an issue or pull request covering:

1. Problem and concrete workload.
2. Current solution and why it is insufficient.
3. Proposed source-level behavior.
4. Proposed IR/runtime semantics.
5. Failure and security implications.
6. Measurement or acceptance criterion.

Syntax without semantics is considered a sketch, not a language proposal.

## Documentation style

- Distinguish current behavior from proposed behavior.
- Prefer precise examples over broad claims.
- Do not claim a performance advantage without a reproducible benchmark.
- Use “must” only for required semantics and “should” for design direction.
- Define new terms in the document or link to the definition.

## Development

There is no compiler toolchain yet. Until code is introduced, verify that Markdown links are valid, examples carry their experimental status, and design documents remain consistent with the principles in `docs/design.md`.

## License

By contributing, you agree that your contributions will be licensed under the Apache License 2.0.
