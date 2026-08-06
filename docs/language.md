# Illustrative text projection

This file demonstrates one possible textual view of Eve. It is not the language's source of truth, a grammar, or a compatibility promise.

The authoritative program is the canonical Eve Graph described by [RFC-0001](../rfcs/0001-eve-language-kernel.md). Parsing text proposes a graph transaction; printing a graph produces canonical text. Structural AI tools may edit the graph without passing through text at all.

## A minimal flow

```eve
type Prompt {
    id: u128
    tokens: Tensor<u32>[n]
}

type Completion {
    id: u128
    tokens: Tensor<u32>[m]
}

network inference {
    node gateway: cpu
    node model: accelerator(memory >= 80GiB)

    stream requests: Prompt from gateway to model {
        ordered by .id
        capacity 1024
        deadline 20ms
        delivery at_most_once
    }

    stream results: Completion from model to gateway {
        ordered by .id
        capacity 1024
        deadline inherits
        delivery at_most_once
    }
}
```

The program declares logical nodes and stream constraints. It does not hard-code IP addresses, ports, or a transport. Those appear in the deployment inventory and execution plan.

## Stateful cells

```eve
cell Router(state: RouteTable) {
    receive request: Request {
        let target = state.choose(request.kind)
        send request to target
            or timeout after 3ms
    }

    receive health: Health {
        state = state.update(health)
    }
}
```

A cell processes one state transition atomically unless a declaration opts into parallel state partitions. Sending is an effect and can fail according to the stream contract.

## Placement

```eve
place Router {
    replicas 3..8
    within region("eu-west")
    separate failure_domain("host")
    scale when queue.p95 > 64 for 5s
}
```

Placement is constraint-based. A program may specify an exact device only when it truly depends on that device.

## Tensor-aware communication

```eve
stream activations: Tensor<bf16>[batch, sequence, hidden]
    from encoder[*] to decoder[*] {
        shard by batch
        preserve layout
        prefer zero_copy
        fallback copy
    }
```

`prefer` expresses an optimization with a valid fallback. A hard requirement uses `require`, and compilation fails when the target inventory cannot satisfy it.

## Explicit remote failure

```eve
fn score(candidate: Candidate) -> Result<Score, unavailable | timeout>
    remote
    deadline 2s
    retry exponential(max: 2) when unavailable
```

Retries require an idempotent operation or an explicit deduplication key. The compiler rejects a retry policy it cannot reconcile with the function's effects.

## Capabilities

```eve
capability evaluation_data: read Dataset<Evaluation>
capability candidate_output: append CandidateLog

cell Evaluator
    with evaluation_data, candidate_output
    budget {
        gpu_time <= 20min
        network_egress = 0B
    }
```

Capabilities are unforgeable runtime handles represented statically in the checked program. A child receives no ambient access by default.

## Evolution

```eve
population policy: Model<Policy> {
    parent stable:v42
    variants 16
}

evolve policy {
    mutate weights(rate: 0.01)
    mutate structure using approved_transforms

    evaluate each on suite("policy-v3")

    promote candidate when {
        candidate.reward > parent.reward * 1.02
        candidate.safety >= parent.safety
        candidate.memory <= 24GiB
    }

    rollout canary(5%) for 30min
    rollback when error_rate > parent.error_rate * 1.1
}
```

Mutation does not bypass normal compilation, capability checking, or deployment policy.

## Canonical form

The compiler should expose:

```text
eve format --canonical program.eve
eve check --diagnostic-format json program.eve
eve ir emit --version 0 program.eve
eve plan --inventory cluster.json program.eve
eve simulate --fail node=model-2 program.eve
eve graph query --cell model --include effects,capabilities program.evegraph
eve graph apply --base <content-id> patch.evepatch
```

Exact commands will be chosen when a prototype exists. The important property is that formatting, diagnostics, IR production, and failure simulation are deterministic APIs rather than editor-only conveniences.
