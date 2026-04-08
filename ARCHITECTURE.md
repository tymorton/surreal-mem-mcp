# Architecture & Technical Tradeoffs

`surreal-mem-mcp` is designed to be the definitive "Edge-RAG Memory Daemon" for autonomous coding agents (like Claude Desktop, Openclaw, Nemoclaw, and Gemini CLI). To achieve sub-millisecond graph traversals and minimize LLM token bloat, this server diverges significantly from traditional AI application architectures.

Here is a deep dive into the engineering decisions governing the system.

---

## 1. Zero-LLM Information Retrieval
A massive problem in the current RAG ecosystem is the over-reliance on "LLM-as-a-Judge" rerankers. Sending 50 initial vector search results to an LLM to determine the "top 5" consumes excessive input tokens and introduces massive latency overhead.

`surreal-mem-mcp` bypasses LLMs entirely during the retrieval phase via a **Bayesian Math Outer Query**:
- **70% Weight:** `vector::similarity::cosine()` (Semantic meaning via Jina ONNX embeddings)
- **30% Weight:** Full-Text Search BM25 (Exact keyword matching)
- **Epistemic Multipliers:** The raw score is multiplied against the node's `access_count` (how useful it has been historically) and a Time Decay logarithm (to prioritize newer context natively).

By forcing the Rust daemon to compute these math vectors instantaneously, the Model Context Protocol only returns highly-distilled, mathematically-ranked context back to the AI. This severely cuts down on token consumption and speeds up inference loops.

## 2. SurrealDB (Graph) vs. SQLite (Relational)
Most local AI agents default to SQLite for storage. While relational databases are phenomenal for tabular data, they struggle immensely with hierarchical reasoning.

When an AI needs to understand context 5 steps removed *(e.g. "What module imported the function that called the API that crashed?")*, a relational database requires `N+1` recursive SQL loops—where the agent asks for a file, reads the code, asks for the next file, reads the code, etc. This burns hundreds of thousands of tokens.

**Surreal-Mem-MCP previously embedded RocksDB, but now operates as a Centralized WebSocket Daemon (`ws://127.0.0.1:24556`)**:
- Running a single centralized SurrealDB engine eliminates file-locking conflicts when multiple orchestrators (e.g., Cursor, Antigravity, and Claude) attempt to access the memory graph concurrently.
- Graph Traversal allows the `search_memory_graph` tool to instantly run a native 5-Hop query: `SELECT * FROM (SELECT <-contains<-file<-imports<-file FROM $match)`.
- The database traverses these edges in `~67.88µs`, returning a single, synthesized JSON map of the entire causal chain to the LLM in *one* tool execution.
- **Tradeoff:** The dedicated local graph processes require a slightly higher active compute footprint. However, separating the Rust Axum SSE Proxy from the database engine provides total concurrent concurrency and stability across massive multi-agent flows without locking files.

## 3. Deterministic Graph Hashing (AST Indexing)
When parsing local codebases into AST (Abstract Syntax Tree) Graph nodes, most naive systems generate random UUIDs (`Uuid::new_v4()`) for every inserted class, function, and file.
- **The Issue:** Tracking ghost nodes. If you restart the indexer, it will create *new* IDs for identical code, creating a tangled, duplicated mess of memory context. If you rename a file, the old UUID remains forever orphaned.

**Our Solution (Phase 3 Syncing):**
Instead of UUIDs, `surreal-mem-mcp` generates string-based identities using `std::hash::DefaultHasher`. 
- Every file's ID is deterministically locked to its absolute path.
- Every function and class ID is locked to its `(Path + Symbol + Line Boundary)`.

When the AST extractor finishes its pass via the `index_codebase` tool, it drops any graph node that no longer matches the active Hasher index. This makes the indexer entirely **IDempotent**—it can be run safely a thousand times without duplicating memory or tearing graph edges!

## 4. Multi-Agent Scoping Layers
Because `surreal-mem-mcp` is hosted externally to the IDE, we built isolated graph scopes to prevent agent cross-contamination.
- **`session`**: Ephemeral scratchpad memory. Garbage collected automatically when the IDE is closed via the `end_session` MCP tool.
- **`agent`**: Specialized context for specific workflows *(e.g., the DevOps agent doesn't need to read CSS graph data)*.
- **`global`**: Promoted, universally recognized rules (writing paradigms, user preferences) stored in the `SOUL.md` and `MEMORY.md` roots.

This hierarchical scoping natively mirrors the architecture of highly autonomous multi-agent frameworks like `Openclaw` and `Nemoclaw`.

## 5. Semantic Redaction & Data Privacy
A critical security flaw in early agentic memory systems (like native Code Puppy or older Gemini CLIs) is the passive logging of sensitive environment variables to local databases. If an agent unknowingly reads an `.env` file, the `sk-proj-...` keys become permanently written to disk.

To achieve enterprise-grade data-at-rest security, `surreal-mem-mcp` utilizes a **Semantic Redaction** engine.
- Before any memory block or AST node is ingested into the graph, a pre-compiled, highly-optimized Rust regex suite (`lazy_static`) intercepts the payload.
- It scans the payload for OpenAI keys, AWS keys (Access & Secret), Stripe keys, GitHub PATs, Google Cloud tokens, Anthropic APIs, and Credit Cards.
- Instead of destructively dropping the block, the engine Semantically Redacts the context. For example, `sk-proj-1234` becomes `<REDACTED_OPENAI_API_KEY>`.

This allows the Database volume to remain 100% free of plaintext secrets. If an autonomous framework like Openclaw later queries that memory node, it understands exactly *what* was used, and will natively pivot to its own secure environment manager (e.g., Doppler, HashiCorp Vault) to fetch the valid key just-in-time.

## 6. LLM-in-the-Loop Architecture

A common misconception is that `surreal-mem-mcp` relies *only* on BM25 and vector embeddings — a purely mathematical approach. This is not accurate. The system is designed as **"LLM-Guided Math Retrieval"**, where the LLM actively participates in the retrieval cycle at multiple stages without paying the cost of LLM-as-a-reranker.

Here is exactly where the LLM contributes:

| Stage | Who Does It | How |
|---|---|---|
| **Query formulation** | LLM | The LLM writes the natural language query passed to `search_memory`. The quality of the query directly affects vector similarity scores. A better query = more precise retrieval. |
| **Tool selection** | LLM | The LLM decides *when* to call `search_memory` vs `search_memory_graph`, and with what arguments. It selects the right scope, limit, and depth based on context. |
| **Memory categorization** | LLM | When calling `remember`, the LLM chooses what to store, which scope to use, and what to put in the `headline` vs `text` fields. This is the primary editorial intelligence layer. |
| **Scope promotion** | LLM | The LLM decides when to call `promote_memory` to elevate a session-scoped memory to agent or global scope — determining which facts are universally valuable. |
| **Result interpretation** | LLM | After `search_memory` returns ranked results, the LLM reads them, decides which are relevant, and *ignores* the rest. This is implicit post-retrieval reranking at zero additional token cost. |
| **Headline authoring** | LLM | The `headline` field is authored by the LLM at store time, providing a compressed representation that enables lossless context sweeps (see Section 7). |

The math layer (Bayesian fusion) handles **scoring and ranking**. The LLM layer handles **editorial decisions and query intelligence**. Together, they achieve precision that neither could alone.

## 7. Lossless Context Memory

The fundamental challenge in long-horizon agent tasks is **context window management**: how do you maintain full memory fidelity without flooding the token window?

### The Problem
- **Full recall** = inject all retrieved memory text → token bloat → context overflow → degraded reasoning
- **Hard truncation** = cut memory to fit → loss of nuance → hallucination and continuity breaks
- **Goal**: Return only what's needed, at the resolution actually needed, every time.

### Dual-Representation Storage
Each memory stored via `remember` can contain two representations:

```
┌─────────────────────────────────────────────────────────┐
│ MEMORY RECORD                                           │
│                                                         │
│ text:     "The JWT authentication middleware was broken  │
│            in prod because the RS256 public key was     │
│            rotated without updating the .env. Fixed by  │
│            setting AUTH_PUBLIC_KEY in the k8s secret.   │
│            Also updated the rotation runbook."          │
│                                                         │
│ headline: "JWT auth failed in prod: RS256 key rotation  │
│            not synced to k8s secrets. Fixed."           │
│                                                         │
│ embedding:          [full text vector, 768-dim]         │
│ headline_embedding: [headline vector, 768-dim]          │
└─────────────────────────────────────────────────────────┘
```

The `text` is full-fidelity. The `headline` is a 1-2 sentence compressed summary authored by the LLM at store time.

### Two Retrieval Modes

**Mode 1: Full-Fidelity** (`search_memory` default, `compressed=false`)
- Returns `text` for all results
- Use when you need precise details, code references, or full context
- Higher token cost, maximum resolution

**Mode 2: Compressed / Lossless** (`search_memory` with `compressed=true`)
- Returns `headline` summaries instead of full `text`
- 5-10x lower token consumption for typical memories
- The LLM reads the headlines, identifies which memories are actually relevant, then calls `search_memory_graph` on *those specific IDs* for full fidelity
- This two-step pattern — sweep compressed, expand precise — is how human memory actually works

### Recommended Agent Pattern

```
1. Session start:
   search_memory(query, compressed=true, limit=15)
   → Get 15 headline summaries cheaply. Identify the 2-3 that matter.

2. Precision recall:
   search_memory_graph(specific_query, max_depth=5)
   → Deep-traverse the graph from the relevant memory root. Get full context.

3. During session work:
   remember(text=<full detail>, headline=<1-2 sentence summary>)
   → Always provide both representations for future lossless retrieval.
```

This pattern maintains full memory resolution while keeping the active context window lean — the definition of lossless context memory.

## 8. Dynamic Tool Proxying & Temporal Knowledge Graph (TKG) Telemetry

As `surreal-mem-mcp` scales into an autonomous multi-agent gateway, robust tool observability and safety become paramount. Direct execution of unsandboxed capabilities or untracked API calls reduces reliability and limits iterative agent self-healing. To solve this, the server utilizes a **Sandboxed Execution Proxy Layer** integrated with a **Temporal Knowledge Graph (TKG)**.

### The Execution Proxy Layer
When an agent invokes a tool (e.g., executing a script, calling an API, or passing through to downstream MCPs), the request is intercepted by the proxy layer (`registry/proxy.rs`). 
- **Security Guardrails**: All `local_script` executions explicitly bypass system shells (`sh -c`) by passing arguments via `tokio::process::Command::args()`. Environment variables are injected defensively, eliminating script injection and path traversal vectors.
- **Protocol Normalization**: The proxy abstracts the underlying RPC mechanism. Whether the capability is a REST API or a nested JSON-RPC MCP server (`std::process::Stdio`), the agent receives a homogenous response.

### TKG Telemetry Instrumentation
As every capability is evaluated, the proxy records a Temporal Edge inside SurrealDB tracking its lifecycle.

```sql
RELATE session:$current_session->EXECUTED->capability:$tool_id SET 
    success = true, 
    duration_ms = 145, 
    timestamp = time::now();
```

By logging these executions as graph edges, `surreal-mem-mcp` automatically builds a deep behavioral history of *how* tools are utilized. If an LLM needs to know "Which tools failed over the past hour?", it is a sub-millisecond graph query.

### Self-Healing & Discovery Degradation
Agent pipelines frequently break when external APIs time out or underlying scripts fail. 
Our TKG implements an active **Degradation Check**:
1. After every execution, the proxy inspects the past 3 `EXECUTED` edges for that tool.
2. If all 3 resulted in `success = false`, the tool's `status` in the registry is updated to `'degraded'`.
3. During `discover_capabilities` (the MCP `tools/list` endpoint), the server intercepts the tool description for any degraded tool and dynamically prepends: 
   `[⚠️ DEGRADED: This tool has failed its last 3 executions. Use with caution or attempt to fix the underlying issue.]`

This contextual injection ensures the LLM is acutely aware of failing tools *before* it attempts to use them, enabling true autonomous self-healing and dynamic pathfinding without hardcoding error fallbacks into the prompt.
