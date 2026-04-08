---
name: surreal-mem-mcp
description: Advanced guidelines and workflows for leveraging the surreal-mem-mcp (SurrealDB Memory Proxy) capabilities. Use this skill when you need to perform cross-session knowledge retention, index local codebases into the Bayesian graph, search for deeply interconnected context, implement long-term agent memories, or route API/script tools dynamically through the proxy server. Essential for effectively using the suite of 14 memory tools.
---

# Surreal-Mem-MCP Operating Guidelines

This skill provides the architectural logic for interacting with the `surreal-mem-mcp` memory store safely and optimally. Treat the memory proxy as a long-term Bayesian graph linking knowledge, code, tools, and past session history.

## Workflows

### 1. Codebase Indexing
When instructed to index a codebase or parse directory structures visually into the graph:
1. Always start by calling `check_index_status` on the target root directory.
2. If `indexed: false` or if you need to refresh stale files, execute `index_codebase` passing the absolute directory path.
3. This process leverages local tree-sitter AST parsing; do not index massively heavy vendor or `.git` directories directly.

### 2. Deep Context Retrieval
When you encounter a task that feels related to unprovided previous sessions:
1. Always run `search_memory(compressed=true)` first! The `compressed=true` flag prevents context-window bloat by returning only lightweight headline properties, giving you broad oversight of hundreds of nodes.
2. If the results contain a memory node that looks explicitly relevant, pivot to `search_memory_graph` utilizing the node's scope/keywords to run multi-hop (3rd degree) relationship traversal, pulling in any implicitly linked data.

### 3. Emitting Semantic Memory
When you solve a complex architectural bug or learn a profound fact about the user's local hardware or system:
1. Formulate a dense but precise `headline` summarization. (e.g., *Fixed memory bug by migrating RocksDB client bound to Any.*)
2. Include the verbose details inside `text`.
3. Set the scope properly. `session` is ephemeral (<24h TTL) while `agent` and `global` dictate persistence.
4. Call `remember` strictly whenever you reach a major milestone, error resolution, or pipeline integration phase. 
5. If an existing temporary session memory suddenly becomes profound, upgrade it by invoking `promote_memory` to lock it persistently into the `global` graph.

### 4. Dynamic Tool Routing & Skill Building
The `surreal-mem-mcp` daemon also acts as an LLM capability orchestrator. 
- Use `discover_capabilities` with your natural language intent to check if the daemon already knows how to solve your goal.
- If it returns tools, use `get_skill_runbook` to pull the precise execution context needed.
- If directed to teach the agent a new capability, use `learn_skill` to persist the prompt behavior and tool sequences directly into the registry for future swarm agents to leverage! 
