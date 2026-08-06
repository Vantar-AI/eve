# Prior art and intended gap

Eve should reuse existing work and earn every new abstraction. No single comparison below is a direct competitor; each solves part of the problem.

| Area | Examples | What Eve should learn or reuse | Intended difference |
|---|---|---|---|
| Content-addressed languages | Unison | Definitions identified by syntax-tree content; names as metadata | Extend content identity across distributed contracts, plans, and evolutionary history |
| Structural/live programming | Hazel/Hazelnut | Typed holes, meaningful incomplete programs, typed edit actions | Make structural editing native to AI-authored distributed graphs |
| Fault-tolerant actors | Erlang/OTP, Pony, Orleans | Supervision, isolation, messaging, capabilities | Tensor-aware placement, data movement, and evolutionary contracts |
| Protocol languages | P | Asynchronous state machines and systematic distributed-protocol testing | Integrate protocol behavior with placement, resources, content identity, and evolution |
| Distributed/HPC languages | Chapel, Legion, Regent, MPI | Locality, partitioning, collectives, performance models | AI-specific types plus persistent, changing service graphs |
| AI compiler IRs | MLIR, StableHLO, IREE | Progressive lowering and hardware portability | Communication, deployment, failure, and evolution as program semantics |
| AI distribution | PyTorch DTensor, JAX sharding, XLA SPMD | Logical tensors, meshes, automatic collective insertion | Extend beyond one model graph into the surrounding server system |
| GPU/server communication | NCCL, NVSHMEM, UCX, libfabric | Optimized transports and collective implementations | Compile declarative constraints into these mechanisms |
| RPC and schemas | gRPC/Protobuf, Cap'n Proto, FlatBuffers | Versioned schemas and efficient encoding | Streams with placement, effects, bulk tensors, and runtime planning |
| Dataflow systems | Ray, Flink, Naiad/Timely Dataflow | Distributed scheduling, backpressure, state, recovery | A compiled language with explicit effects and governed program variation |
| Service orchestration | Kubernetes, Nomad | Resource inventory, isolation, lifecycle management | Semantic knowledge of models, tensors, streams, and evaluation gates |
| Programmable networks | P4, eBPF | Safe specialization close to the data plane | An optional lowering target rather than the application language itself |
| Agent protocols | MCP, A2A | Capability discovery and higher-level interoperability | Eve targets execution and data movement inside distributed AI systems |
| Equality graphs | egg, egglog | Represent many equivalent programs and extract by cost | Separate proved/validated optimization from behavior-changing evolution |

## The proposed gap

Eve's intended contribution is the combination of:

- a typed distributed program spanning model and service boundaries;
- first-class cost and failure semantics for communication;
- topology-aware compilation into existing runtimes and transports;
- a canonical representation designed for safe machine-authored change;
- governed evolution with provenance, evaluation, rollout, and rollback.

If this combination can be expressed cleanly as libraries and configuration in an existing language, a new language is unnecessary. The prototype roadmap is designed to test that possibility early.

## Starting references

- [MLIR](https://mlir.llvm.org/)
- [Unison's content-addressed code](https://www.unison-lang.org/docs/the-big-idea/)
- [Hazel typed-hole environment](https://hazel.org/)
- [P language semantics](https://p-org.github.io/P/advanced/psemantics/)
- [egg equality saturation](https://egraphs-good.github.io/egg/egg/tutorials/_01_background/)
- [StableHLO](https://openxla.org/stablehlo/)
- [IREE](https://iree.dev/)
- [MPI Forum](https://www.mpi-forum.org/)
- [UCX](https://openucx.org/)
- [NVIDIA NCCL](https://docs.nvidia.com/deeplearning/nccl/)
- [NVSHMEM](https://docs.nvidia.com/nvshmem/)
- [P4](https://p4.org/)
- [PyTorch DTensor](https://docs.pytorch.org/docs/stable/distributed.tensor.html)
- [Model Context Protocol](https://modelcontextprotocol.io/)
- [Agent2Agent Protocol](https://a2a-protocol.org/)
