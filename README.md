# Surreal-Mem-MCP

A standalone Edge-RAG Memory Daemon utilizing HTTP/SSE MCP transport. The architecture consists of a high-performance Rust core (SurrealDB/RocksDB + Axum server) and a zero-config Python CLI Bootstrapper. It bypasses heavy LLM-as-a-Judge rerankers in favor of an ultra-fast sub-millisecond Bayesian Math Outer Query combining Cosine Similarity and BM25 Posterior scores.

This Model Context Protocol (MCP) server allows your AI agents (Claude Desktop, Cursor, Gemini CLI, OpenCode, Code Puppy, etc.) to use a shared global memory system, complete with standardized behavioral rules (`SOUL.md` and `MEMORY.md`), independent of your current project directory.

## Features

- **Blazing Fast**: Uses Rust, Axum, and native Server-Sent Events (SSE) alongside embedded SurrealDB backed by RocksDB.
- **Embedded ONNX Models**: Runs sovereign `JinaEmbeddingsV2BaseEN` natively in-process out of the box. Zero python required, zero Ollama required, zero external dependencies.
- **Bayesian Edge-RAG**: Blends `vector::similarity::cosine() * 0.7` and `BM25 * 0.3`, weighted by an epistemic prior (time decay, graph density, access counts). Let math do the parsing, not latency-heavy LLMs.
- **Global Behavioral Rules**: Generates `~/.surreal-mem-mcp/rules/` accessible universally via `resources/list`. Memory rules dynamically persist across completely disparate agent ecosystems.
- **Enterprise Semantic Redaction**: Intercepts and scrubs API keys (OpenAI, AWS, GCP, Anthropic, Stripe) and PII natively in Rust before they are written to the database, ensuring your local RocksDB volume is 100% leak-proof.

> **💡 Curious about the technical tradeoffs?** Read **[ARCHITECTURE.md](./ARCHITECTURE.md)** to see how our Epistemic Math Queries drastically drop LLM token consumption, and why our embedded Graph Database outperforms traditional SQLite for dense code reasoning.

## Installation & Setup

Since the daemon bundles the fastembed-rs ONNX runtime, it requires zero external dependencies and zero configuration.

### Manual Setup
You can build the MCP server from source:

```bash
cargo build --release
```

Run the server:

```bash
./target/release/surreal-mem-mcp
```

The server will begin streaming SSE connections locally on port `3000`. By default, the embedded 8k-context Jina ONNX model weights (`~150MB`) will be automatically downloaded and cached locally on first boot.

## Data Storage & Database Location

By default, the SurrealDB / RocksDB graph data volume is saved to your user's home directory across all operating systems.
All memories, configuration files, and indexed AST data reside in:
`~/.surreal-mem-mcp/`

Specifically, your live RocksDB store is located within:
`~/.surreal-mem-mcp/memory_store/`

Because this is a global store, your AI agent will retain its memory context and AST indices regardless of the current IDE workspace you operate inside. If you ever need to completely purge your memory graph and start over, you can simply delete the `memory_store` directory and restart the daemon.

## Visualizing Your Memory Graph

**[Surrealist](https://surrealdb.com/surrealist)** is the official SurrealDB GUI and the recommended way to explore and visualize your agent's memory graph and AST code indices.

### Setup
1. Download [Surrealist](https://surrealdb.com/surrealist) for your OS.
2. Create a new connection with the following settings:
   - **Connection type:** `Local / Embedded RocksDB`
   - **Database path:** `~/.surreal-mem-mcp/memory_store`
3. Open the **Explorer** or **Query** tabs to interactively inspect your data.

### Useful Queries

Browse all stored memories:
```sql
SELECT id, text, scope, created_at, access_count FROM memory ORDER BY access_count DESC;
```

Visualize the knowledge graph connections:
```sql
SELECT id, text, ->related_to->memory.text AS connections FROM memory LIMIT 20;
```

Browse your indexed AST code graph (files, functions, classes):
```sql
SELECT * FROM file;
SELECT * FROM func;
SELECT * FROM class;
```

Explore function call relationships:
```sql
SELECT name, ->calls->func.name AS calls_functions FROM func LIMIT 50;
```

> 💡 **Tip:** Surrealist's **Graph View** tab will render your `->related_to->` and `->calls->` relationship edges as an interactive visual graph — no extra tooling required.

## MCP Capabilities

### Tools

- `remember`: Store a memory with its embeddings in the Bayesian Graph memory store. Requires a `scope` (global, agent, session) and optional `agent_id` and `session_id`.
- `search_memory`: Search the memory store using Bayesian Fusion (70% Vector + 30% BM25) and Epistemic Uncertainty checks. Enforces strict scope boundaries.
- `search_memory_graph`: Perform a deep 5-hop knowledge graph traversal starting from the most relevant memory match, avoiding latency-heavy repeated SQL execution.
- `promote_memory`: Graduate a highly valuable `session` or `agent` memory to the `global` scope while preserving its graph edges and access frequency multipliers.
- `update_behavioral_rules`: Append or rewrite the learned user preferences in the dynamic `MEMORY.md` file.
- `end_session`: Instantly garbage collect and prune ephemeral `session` scoped memories from the RocksDB instance to prevent context bloat.

### How Scoping Works (Orchestrator Integration)
The human user **never** calls these parameters manually. The AI model autonomously invokes the tools via MCP. For the scoping logic to function perfectly, your Orchestrator (e.g., OpenCode, Gemini CLI, Code Puppy, custom LangChain pipelines) must inform the LLM of its current context:
1. **Prompt Injection:** Inject the current identifiers into the agent's system prompt *(e.g. "Your agent_id is 'tech_writer', your session_id is '9b1deb4d...'. Pass these to memory tools.")*
2. **Middleware Interception:** Alternatively, the orchestrator can intercept the LLM's tool execution payload and forcefully inject/override the `agent_id` and `session_id` before transmitting it to the Surreal-Mem-MCP daemon.

> **Building with LangChain, Openclaw, Nemoclaw, CrewAI, or LlamaIndex?** 
> See our explicit **[Multi-Agent Framework Integration Guide (FRAMEWORKS.md)](./FRAMEWORKS.md)** to see how to mount these memory tools into code-based orchestration frameworks.

### Resources

- `memory://rules/soul`: Exposes the immutable Core Identity rules guiding the AI agent.
- `memory://rules/learned`: Exposes dynamic Working Memory preferences the AI observes from interactions.

## Benchmarks & Tradeoffs
Traditional autonomous agent platforms inherently rely on SQLite. By embedding **SurrealDB** mapped down to **RocksDB** underneath an Axum SSE server, we benchmarked the following capabilities:

- **Deep Traversal (5 Hops):** `~67.88µs` resolving deeply connected concepts autonomously. SQLite lacks naive wide-graph recursive structures and would drop to `N+1` loops to achieve the same result.
- **Data Density:** RocksDB actively packs its memory trees ~18% more byte-efficiently than SQLite B-Trees.
- **RAM Compute Tradeoff:** Surreal-Mem-MCP averages a larger footprint (`~250.2 MB`) vs generic memory handlers out of process (`~58.0 MB`). This is because the server completely bundles the heavily optimized Jina ONNX model directly in process space, granting zero-latency vectorization without HTTP transport overhead.

## Client Integration

### Openclaw, Nemoclaw & Code Puppy Integration
If you are operating within **Openclaw**, **Nemoclaw**, or using the **Code Puppy** IDE, `surreal-mem-mcp` is deeply supported. Most other agent harnesses based on the Openclaw architecture should work out of the box as well!
- **Code Puppy:** The Python bootstrapper automatically injects the MCP toolset into `~/.code_puppy/mcp_servers.json`.
- **Openclaw / Nemoclaw:** These frameworks natively integrate with SurrealDB via its internal engine API (REST/WS) rather than relying on standard MCP wrappers. Therefore, no local configuration injection is required for Openclaw orchestrators.

### Gemini CLI, OpenCode, Code Puppy, etc.
Here are specific setup instructions for integrating the Memory Daemon into various CLI coding agents. Once configured, each agent will instantly have access to the `remember`, `search_memory`, and `update_behavioral_rules` tools, as well as the global rules resources.

### 1. Code-Puppy

Add the following configuration to `~/.config/code_puppy/mcp_servers.json`:

```json
{
  "mcp_servers": {
    "surreal-mem-mcp": {
      "type": "sse",
      "url": "http://127.0.0.1:3000/sse"
    }
  }
}
```

### 2. OpenCode

OpenCode defines its MCP servers within a global `~/.config/opencode/opencode.json` or project-local `./opencode.json` file. Note that it expects `command` to be an array and requires `type: "local"`:

```json
{
  "mcp": {
    "surreal-mem-mcp": {
      "type": "sse",
      "url": "http://127.0.0.1:3000/sse"
    }
  }
}
```

### 3. Gemini CLI

In your Gemini playbook (`GEMINI.md` / `settings.json`), use the standard MCP schema:

```json
{
  "mcpServers": {
    "surreal-mem-mcp": {
      "type": "sse",
      "url": "http://127.0.0.1:3000/sse"
    }
  }
}
```

### 4. Claude Code

For Anthropic's Claude Code CLI, the easiest configuration method is via the CLI itself. Run this command in your terminal to globally map the daemon:

```bash
claude mcp add surreal-mem-mcp http://127.0.0.1:3000/sse
```

Alternatively, for project-specific mapping, add it manually to your project's `.claude/mcp.json` or global `~/.claude/settings.json`:

```json
{
  "mcpServers": {
    "surreal-mem-mcp": {
      "type": "sse",
      "url": "http://127.0.0.1:3000/sse"
    }
  }
}
}
```

### 5. Google Antigravity

For Google's agent-first IDE, integration requires editing the application's native configuration map.

1. Open the MCP store via the **"..."** dropdown at the top of the editor's agent panel.
2. Click on **"Manage MCP Servers"**.
3. Click on **"View raw config"**.
4. Modify the `mcp_config.json` (which saves to `~/.gemini/antigravity/mcp_config.json`) with the daemon's SSE mapping:

```json
{
  "mcpServers": {
    "surreal-mem-mcp": {
      "type": "sse",
      "url": "http://127.0.0.1:3000/sse"
    }
  }
}
```

## Acknowledgments

This project stands on the shoulders of incredible open-source infrastructure. We explicitly want to thank and credit:

- **[SurrealDB](https://surrealdb.com/)**: For providing the blazing-fast, multi-model graph database engine that makes our 5-hop Edge-RAG traversals possible.
- **[RocksDB](https://rocksdb.org/)**: The embeddable persistent key-value store developed by Meta, heavily powering the core SurrealDB runtime.
- **[CodeGraphContext (CGC)](https://github.com/DevonSystems/CodeGraphContext)**: The original Python/Neo4j implementation that inspired our transition to a lightweight, embedded Rust agentic graph daemon. 

## OS Support Map
Binaries are automatically built for the following architectures:
- macOS (Apple Silicon: `aarch64`)
- Linux (`aarch64` / `x86_64`)
- Windows (`x86_64`)

> ⚠️ **macOS Intel (x86_64):** Pre-built binaries are not provided for Intel Macs due to a build-time incompatibility in the `ort-sys` crate — the ONNX Runtime xcframework bundles cannot be linked for `x86_64-apple-darwin` when compiling on ARM-based CI runners. Intel Mac users should [build from source](./CONTRIBUTING.md) using `cargo build --release`.

> ⚠️ **Windows ARM64 (e.g. Snapdragon X Elite):** Pre-built binaries are not available for this platform in v0.1.0 due to a build-time incompatibility in a transitive dependency. Windows ARM64 users should [build from source](./CONTRIBUTING.md) using `cargo build --release`.
