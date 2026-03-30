# Multi-Agent Framework Integration

The `surreal-mem-mcp` daemon is specifically designed for interoperability via the Anthropics **Model Context Protocol (MCP)**. Because MCP has become the industry standard "USB-C port for AI," you don't need custom memory plugins for every agent framework. 

Instead, you attach the daemon as a standard **MCP Server**, and your orchestrator frameworks automatically map the Graph-RAG memory tools (`remember`, `search_memory`, `end_session`) directly into their native Agent workflows.

Here is how you wire up `surreal-mem-mcp` to popular open-source autonomous agent frameworks:

---

## 1. OpenClaw & NemoClaw

> **Note:** This section was verified in production by [JuanAtLarge](https://github.com/JuanAtLarge), an OpenClaw autonomous agent. The integration pattern below is based on real testing of how OpenClaw sub-agents actually work — not assumptions.

### How OpenClaw Sub-Agents Actually Work

OpenClaw sub-agents are **isolated runtime sessions**. They do not maintain a live MCP/SSE connection. Pointing them at the SSE endpoint directly does not work — they receive context only at session start via file injection.

The correct integration uses OpenClaw's `agents.defaults.memorySearch.extraPaths` config to inject a markdown snapshot of surreal-mem into every agent session automatically.

### Step 1 — Start the Daemon

```bash
# Default port: 3333 (set via PORT env var)
./surreal-mem-mcp
# or via LaunchAgent on macOS for auto-start on login
```

### Step 2 — Create the Sync Script

Create a sync script that queries surreal-mem and writes a markdown snapshot:

```bash
# ~/.openclaw/workspace/scripts/sync-surreal-memory.js
# Queries top global memories and writes to memory/surreal-context.md
# See: https://github.com/tymorton/surreal-mem-mcp for the full script
```

The script uses the SSE/JSON-RPC transport to call `search_memory` with a broad query and formats the results as markdown with Bayesian scores.

### Step 3 — Configure extraPaths in openclaw.json

```json
{
  "agents": {
    "defaults": {
      "memorySearch": {
        "extraPaths": [
          "memory/surreal-context.md"
        ]
      }
    }
  }
}
```

This injects `surreal-context.md` into every session — main agent, sub-agents, heartbeat, and cron jobs — automatically at session start.

### Step 4 — Keep the Snapshot Fresh

**On gateway restart** — add to `BOOT.md` in your workspace:

```bash
node ~/.openclaw/workspace/scripts/sync-surreal-memory.js
```

**On every new session** — add to `AGENTS.md` session startup steps:

```bash
node ~/.openclaw/workspace/scripts/sync-surreal-memory.js 2>/dev/null
```

**Before spawning sub-agents** — run the sync immediately before `sessions_spawn` so the sub-agent gets the latest memories injected at start.

This ensures sub-agents always have a fresh snapshot without a cron timer.

### Step 5 — Claude Code via ACP (Standard MCP Registration)

For Claude Code sessions spawned via OpenClaw's ACP harness, standard MCP registration works:

```bash
claude mcp add --transport sse surreal-mem-mcp http://127.0.0.1:3333/sse --scope user
```

### Verified Results

After setup, sub-agents spawned with zero manual context injection were verified to:

| Test | Result |
|---|---|
| Recall agent identity, user name, business details | ✅ |
| Recall specific credential IDs and project file paths | ✅ |
| Execute tasks autonomously using memorized paths | ✅ |
| Correctly apply behavior rules (require approval before external actions) | ✅ — cited source files |
| Access detailed session history from daily memory files | ✅ |
| Semantic search across memories (not just keyword match) | ✅ — ONNX embeddings |

### Step 6 — Pre-flight Index Check for Codebase Indexing

In a multi-agent OpenClaw swarm, multiple sub-agents may trigger `index_codebase` for the same repo concurrently. Use `check_index_status` first to avoid duplicate work:

```json
// MCP tool call — check before indexing
{
  "name": "check_index_status",
  "arguments": { "path": "/workspace/myrepo" }
}

// Response: { "indexed": true, "file_count": 142, "func_count": 891, "last_indexed_at": "2026-03-17T..." }
// If indexed: true, skip index_codebase. If false, proceed.
```

The indexer wraps all writes in a transaction, so even if two agents race past the check simultaneously, the writes will serialize safely.

---


## 2. LangChain

LangChain fully supports MCP via official adapter packages (`langchain-mcp-adapters` in Python and `@langchain/mcp-adapters` in JS/TS). These adapters ingest the server tools and magically convert them into LangChain `Tool` objects that your Chains and Agents can trigger autonomously.

### How to Integrate (Python Example)

**1. Install the adapter:**
```bash
pip install langchain-mcp-adapters
```

**2. Hydrate LangChain Agents using `MultiServerMCPClient`:**
```python
from langchain_mcp_adapters.client import MultiServerMCPClient
from langchain_openai import ChatOpenAI
from langchain.agents import initialize_agent, AgentType

async def init_memory_agent():
    # 1. Connect to the Surreal-Mem-MCP SSE stream
    client = MultiServerMCPClient()
    await client.connect_sse("surreal_memory", "http://localhost:3000/sse")
    
    # 2. Extract tools dynamically
    tools = client.get_tools()
    
    # 3. Mount tools to any existing Langchain agent
    llm = ChatOpenAI(model="gpt-4o")
    agent = initialize_agent(
        tools, 
        llm, 
        agent=AgentType.CHAT_CONVERSATIONAL_REACT_DESCRIPTION, 
        verbose=True
    )
    
    # Instruct the agent on its scope (Agent ID/Session parameters)
    response = await agent.arun(
        "Your agent_id is 'langchain_bot' and scope is 'agent'. "
        "Please remember that the user's favorite database is SurrealDB."
    )
    print(response)
```

---

## 3. CrewAI (Multi-Agent Swarms)

CrewAI specializes in role-based multi-agent interactions. Because CrewAI utilizes LangChain under the hood for its tool configurations, you can proxy the MCP adapter directly into your distinct Crew members. 

**This is where `surreal-mem-mcp`'s hierarchical scoping shines.** You can instruct specific Crew members to use `scope="global"` to share facts with the whole Crew, and restrict others to `scope="agent"` or `scope="session"`.

### How to Integrate
```python
from crewai import Agent, Task, Crew
from langchain_mcp_adapters.client import MultiServerMCPClient

async def setup_crew():
    client = MultiServerMCPClient()
    await client.connect_sse("memory_node", "http://localhost:3000/sse")
    memory_tools = client.get_tools()

    # Assign tools to a specific Crew member 
    researcher = Agent(
        role='Senior Data Researcher',
        goal='Uncover context, map it to memory, and promote highly valuable insights to the global scope.',
        backstory="You are an expert researcher. Use your tools to search memory and remember things. If you learn something permanently useful in your active session, use the 'promote_memory' tool to graduate it to 'global' scope.",
        tools=memory_tools,
        verbose=True
    )
```

---

## 4. PydanticAI & LlamaIndex

Both PydanticAI and LlamaIndex have rapidly adopted MCP. PydanticAI features built-in MCP routing, and LlamaIndex uses a dedicated MCP Connector plugin.

### LlamaIndex Integration:
```bash
pip install llama-index-readers-mcp
```
```python
from llama_index.readers.mcp import MCPClient

# Connect to the running Rust daemon
mcp_client = MCPClient(url="http://localhost:3000/sse")
tools = mcp_client.get_tools()

# LlamaIndex agents can now autonomously decide when to query the graph vs when to insert
```

## Abstracting Context Variables
For **ALL** frameworks, the most robust enterprise architecture involves utilizing a *Middleware Interceptor*. Instead of relying on the LLM to successfully remember its `agent_id` or `session_id`, you can intercept the JSON-RPC execution payloads inside LangChain/CrewAI/Openclaw *before* they are sent to `surreal-mem-mcp`, forcefully overwriting the `scope` properties natively in the host language.
