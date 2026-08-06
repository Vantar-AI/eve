# Runtime experiment

The first Eve runtime tests one claim: a global conversation can remain semantically identical while its projected endpoints execute through different transport plans.

It is deliberately a reference implementation rather than a performance architecture.

## Execution model

Both roles load the same Conversation v0 graph and independently derive their local endpoint machines. No central coordinator advances the conversation.

For every transition, the sending endpoint:

1. Checks that its projected state permits the action.
2. Creates a versioned Eve Wire envelope.
3. Records the conversation name, experimental semantic hash, state, and sequence.
4. Advances only its local endpoint machine.

The receiving endpoint independently checks all envelope fields and the permitted receive action before advancing. It rejects stale, reordered, graph-incompatible, or protocol-invalid frames.

The SHA-256 value is an experimental semantic identity. It excludes the schema path and annotations, but it is not yet an Eve `ContentId`; canonicalization fixtures and normalization rules must exist before that name is justified.

## Transport plans

### Memory

The memory plan executes client and server endpoints on separate threads connected by channels. Envelopes cross a real serialization boundary rather than being passed as Rust objects.

```bash
cargo run -- demo --transport memory --tokens 5 --cancel-after 2
```

### TCP

The TCP plan uses the same JSON envelope with a four-byte big-endian length prefix. Loopback mode makes transport equivalence easy to test:

```bash
cargo run -- demo --transport tcp --tokens 3
```

The roles can also run as separate operating-system processes:

```bash
# Process 1
cargo run -- serve --listen 127.0.0.1:7878 --tokens 4

# Process 2
cargo run -- connect --server 127.0.0.1:7878 --cancel-after 2
```

Each report includes a semantic trace hash. Equivalent client and server hashes mean both independently projected endpoints observed the same ordered protocol transitions.

## Security and correctness boundary

The prototype currently guarantees only:

- local action checking against the projected endpoint;
- conversation-name and semantic-hash agreement;
- exact state and sequence agreement;
- a maximum envelope size;
- explicit selection and cancellation transitions;
- equivalent semantic traces across the two implemented plans.

It does not yet provide:

- encryption, authentication, or authorization;
- asynchronous multiplexing or flow control;
- retries, reconnects, or failure branches;
- enforcement of declared deadlines;
- structural validation of payloads from Eve type definitions;
- canonical graph normalization;
- bulk tensor transfer or zero-copy buffers;
- transport negotiation or performance optimization.

The TCP server listens on loopback by default because the wire plan is plaintext and unauthenticated.

## Next transport experiment

The next plan should use QUIC without changing the Conversation v0 graph, projected endpoint semantics, or Eve Wire envelope.

The experiment passes when:

1. Memory, TCP, and QUIC executions produce the same semantic trace hash for the same inputs.
2. QUIC authenticates the server and encrypts transport data.
3. Stream resets map to explicit Eve cancellation or failure behavior instead of silently becoming application errors.
4. Measurements separate protocol-machine overhead from serialization and transport overhead.
