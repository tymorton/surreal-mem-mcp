# Changelog

All notable changes to `surreal-mem-mcp` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[0.1.0]: https://github.com/tymorton/surreal-mem-mcp/releases/tag/v0.1.0
