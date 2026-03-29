# Changelog

All notable changes to `surreal-mem-mcp` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

## [0.3.0] - 2026-03-29

### Changed
- **SurrealDB v3 Upgrade**: Migrated the core database engine from SurrealDB `v1.0.0` to `v3.0.5` taking advantage of extreme latency improvements.
- **Tree-Sitter Modernization**: Bumped all tree-sitter grammars and the core parsing library to `v0.26.0` (ABI version 15) to support the latest language syntax specifications.

### Fixed
- **SurrealQL Index Initialization**: Updated full-text indexes and graph relational syntax (`<-(file WHERE id IN ...)`) to conform to rigorous SurrealDB v3 specification requirements.
- **AST Silent Drop Prevention**: Handled latent asynchronous errors via `.check()?` ensuring edge and node creations don't fail silently.
- **Database Upserts**: Replaced generic `UPDATE` statements within the codebase indexer with `UPSERT` statements perfectly matching the new v3 schema constraints.

### Added
- **Embedded Sovereign ONNX Embedding Engine**: Replaced the external HTTP-dependent Ollama/LM Studio embedding client with an in-process `fastembed-rs`-powered `JinaEmbeddingsV2BaseEN` ONNX model. The server now bundles vectorization natively — zero configuration required. No Ollama, no Python, no env vars.
- **8,000 Token Context Window**: Jina Embeddings V2 grants a dramatically larger embedding context compared to legacy truncated models, with 768-dimensional output optimized for retrieval.
- **Automatic Dimensionality Migration**: On startup, `SurrealClient` detects any existing memories whose embedding dimensions do not match 768 (e.g. legacy 1536-d Ollama vectors) and transparently re-embeds them using the new Jina model. Zero user action required when upgrading from previous releases.
- **Openclaw Interceptor Middleware**: Added `X-Agent-Id` and `X-Session-Id` header interpolation to the Axum SSE `message_handler`. If tool calls omit scoping identifiers, the server gracefully falls back to headers, enabling zero-shot LLM memory isolation.
- **REST Lifecycle Hooks**: Added `DELETE /session/:session_id` endpoint. Orchestrators (like Openclaw or Nemoclaw Hands) can eagerly prune ephemeral context upon task completion without requiring the LLM to call `end_session`.
- **Pre-flight Concurrency Check**: Added `check_index_status` MCP tool returning `{ indexed, file_count, func_count, last_indexed_at }`. Agents in high-throughput swarms should call this before indexing a repository to prevent duplicate CPU work.
- **Write Attribution Tracking**: Added `author_agent_id` to the `remember` schema. Memories now explicitly track which sub-agent in a swarm originally authored them, improving multi-agent graph debugging.
- **Global Memory Passive TTL**: Added `ttl_days` to the `remember` schema. Global and Agent scoped memories can now opt-in to a passive eviction lifecycle (computed as `expires_at`).

### Fixed
- **SurrealQL `IS NOT NONE` Comparisons**: Fixed four invalid `!= NONE` expressions across `surreal_client.rs` and `indexer.rs`. SurrealQL requires the `IS NOT NONE` operator for null checks in WHERE clauses and conditional blocks. Using `!= NONE` produced incorrect filter behavior — the `embedding IS NOT NONE` vector search filter would pass on records without embeddings, and the `expires_at` TTL eviction would never fire.
- **SurrealQL `IF` Block Syntax**: Fixed two `IF $src != NONE THEN ... END` blocks in the AST indexer's graph edge creation. SurrealQL's modern syntax uses curly-brace blocks (`IF condition { }`), not `THEN/END`. Both have been corrected and validated against the Context7 SurrealDB documentation.
- **AST Relational Edge Duplication**: Fixed a swarm concurrency bug where multiple agents indexing the same codebase simultaneously would duplicate `->contains->`, `->calls->`, and `->imports->` graph edges. Refactored the tree-sitter generation loop to use deterministic idempotent Record IDs (`UPDATE contains:⟨filehash_funchash⟩`) instead of `RELATE`.
- **SSE Transport Diagnostic**: Fixed silent connection hangs for frameworks expecting standard STDIO transport. `GET /` now returns an HTTP 200 health payload explicitly stating `transport: sse` and the exact connection URIs.

### Removed
- **External Embedding HTTP Client**: Removed `reqwest` and the `OpenAiClient` entirely. HTTP-based embedding calls to local inference servers are no longer required or supported.
- **Python Bootstrapper CLI** (`cli/surreal-memory-init.py`): Removed the Python configuration script since the daemon is now fully zero-configuration. Embedding model setup and endpoint probing are no longer necessary.

---


## [0.1.0] - 2026-03-17

### Added
- **Edge-RAG Memory Daemon**: High-performance Rust core using SurrealDB (RocksDB backend) and Axum SSE server.
- **Bayesian Math Retrieval**: Combines `cosine similarity * 0.7 + BM25 * 0.3` with epistemic priors (time decay, access frequency, graph density) for sub-millisecond ranked retrieval without LLM rerankers.
- **MCP Tools**: `remember`, `search_memory`, `search_memory_graph`, `promote_memory`, `update_behavioral_rules`, `end_session`.
- **Multi-Scope Memory**: `global`, `agent`, and `session` scoped memory with strict boundary enforcement.
- **5-Hop Graph Traversal**: Deep knowledge graph traversal via SurrealDB native graph SQL (`~67.88µs`).
- **Multi-Language AST Indexing**: Tree-sitter-based code indexer supporting 14 languages (Python, Rust, JS, TS, Go, Java, C, C++, C#, Ruby, PHP, Swift, and more) via the `index_codebase` MCP tool.
- **Deterministic Graph Hashing**: Idempotent AST re-indexing using `std::hash::DefaultHasher` — eliminates duplicate nodes and stale ghost data.
- **Semantic Redaction**: Pre-commit regex engine strips API keys (OpenAI, AWS, GCP, Anthropic, Stripe, GitHub PATs) and credit card numbers before any data reaches RocksDB.
- **Session TTL Cleanup**: Automatic 24-hour garbage collection for orphaned `session` scope memories to prevent graph bloat.
- **Token Context Guard**: `search_memory_graph` truncates responses at 100,000 characters to protect LLM context windows.
- **Binary File Guard**: AST indexer gracefully skips non-UTF-8 and binary files without crashing the batch job.
- **Python Bootstrapper** (`cli/surreal-memory-init.py`): Zero-config setup script that detects platform/arch, downloads the correct pre-built binary from GitHub Releases, configures embedding model endpoints, and auto-injects MCP config into Claude Desktop, Gemini CLI, Google Antigravity, and OpenCode.
- **Global Behavioral Rules**: Auto-generates `SOUL.md` and `MEMORY.md` rule files in `~/.surreal-mem-mcp/rules/`.
- **ARCHITECTURE.md**: Deep technical writeup covering Bayesian retrieval, graph vs. SQLite tradeoffs, deterministic hashing, multi-agent scoping, and semantic redaction.
- **BENCHMARKS.md**: Quantitative performance comparison vs. SQLite for graph traversal latency and storage density.
- **FRAMEWORKS.md**: Integration guide for Openclaw, Nemoclaw, Claude Desktop, Gemini CLI, OpenCode, LangChain, CrewAI, and LlamaIndex.

### Security
- Semantic Redaction engine blocks API keys, PII, and payment data from persisting to the local database volume.

### Known Issues
- **Windows ARM64 not supported in v0.1.0**: Pre-built binaries are not provided for Windows on ARM64 devices (e.g. Snapdragon X Elite laptops). This is caused by a build-time incompatibility in the `ring` crate (a transitive dependency of SurrealDB's TLS stack) when cross-compiling to `aarch64-pc-windows-msvc` on GitHub's CI runners. Windows ARM64 users should build from source using `cargo build --release` inside the `surreal-mem-mcp` directory. This will be resolved in a future release when an updated version of `ring` with ARM64 Windows support is available.

[0.1.0]: https://github.com/tymorton/surreal-mem-mcp/releases/tag/v0.1.0
