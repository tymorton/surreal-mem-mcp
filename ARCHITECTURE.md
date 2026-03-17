# Architecture & Technical Tradeoffs

`surreal-mem-mcp` is designed to be the definitive "Edge-RAG Memory Daemon" for autonomous coding agents (like Claude Desktop, Openclaw, Nemoclaw, and Gemini CLI). To achieve sub-millisecond graph traversals and minimize LLM token bloat, this server diverges significantly from traditional AI application architectures.

Here is a deep dive into the engineering decisions governing the system.

---

## 1. Zero-LLM Information Retrieval
A massive problem in the current RAG ecosystem is the over-reliance on "LLM-as-a-Judge" rerankers. Sending 50 initial vector search results to an LLM to determine the "top 5" consumes excessive input tokens and introduces massive latency overhead.

`surreal-mem-mcp` bypasses LLMs entirely during the retrieval phase via a **Bayesian Math Outer Query**:
- **70% Weight:** `vector::similarity::cosine()` (Semantic meaning via Nomic embeddings)
- **30% Weight:** Full-Text Search BM25 (Exact keyword matching)
- **Epistemic Multipliers:** The raw score is multiplied against the node's `access_count` (how useful it has been historically) and a Time Decay logarithm (to prioritize newer context natively).

By forcing the Rust daemon to compute these math vectors instantaneously, the Model Context Protocol only returns highly-distilled, mathematically-ranked context back to the AI. This severely cuts down on token consumption and speeds up inference loops.

## 2. SurrealDB (Graph) vs. SQLite (Relational)
Most local AI agents default to SQLite for storage. While relational databases are phenomenal for tabular data, they struggle immensely with hierarchical reasoning.

When an AI needs to understand context 5 steps removed *(e.g. "What module imported the function that called the API that crashed?")*, a relational database requires `N+1` recursive SQL loops—where the agent asks for a file, reads the code, asks for the next file, reads the code, etc. This burns hundreds of thousands of tokens.

**Surreal-Mem-MCP embeds SurrealDB (backed by high-performance RocksDB)**:
- Graph Traversal allows the `search_memory_graph` tool to instantly run a native 5-Hop query: `SELECT * FROM (SELECT <-contains<-file<-imports<-file FROM $match)`.
- The database traverses these edges in `~67.88µs`, returning a single, synthesized JSON map of the entire causal chain to the LLM in *one* tool execution.
- **Tradeoff:** The embedded RocksDB graph requires a higher active RAM footprint (`~155MB`) compared to standard SQLite deployments (`~58MB`). We determined the exponential token savings justified the baseline memory reservation.

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
