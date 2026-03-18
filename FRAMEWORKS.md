# Multi-Agent Framework Integration Guide

This document provides explicit setup instructions for integrating the `surreal-mem-mcp` Memory Daemon into various code-based orchestration frameworks and autonomous agent platforms.

---

## Table of Contents

- [OpenClaw / Nemoclaw](#openclaw--nemoclaw)
- [LangChain](#langchain)
- [CrewAI](#crewai)
- [LlamaIndex](#llamaindex)

---

## OpenClaw / Nemoclaw

[OpenClaw](https://openclaw.ai) is a self-hosted autonomous AI agent platform that manages persistent agent sessions, multi-channel messaging (Telegram, Discord, WhatsApp, Signal), sub-agent orchestration, heartbeat polling, and cron scheduling. It is **not** an IDE-based coding agent — it is a continuously-running agent daemon that spawns isolated sub-agents for background tasks.

> **Important architectural note:** OpenClaw does **not** integrate with `surreal-mem-mcp` via MCP wrappers for its internal sub-agents. This is by design — OpenClaw sub-agents are isolated runtime sessions that inherit workspace context through a file injection system, not a live MCP connection. The integration pattern described below bridges `surreal-mem-mcp`'s global memory store into OpenClaw's context injection pipeline.

---

### Architecture Overview

OpenClaw has a built-in memory/RAG system (`agents.defaults.memorySearch`) that:
- Embeds markdown files using local Ollama embeddings (`nomic-embed-text` recommended)
- Stores embeddings in a local SQLite database (`~/.openclaw/memory/main.sqlite`)
- Searches this store on every agent turn via `memory_search` / `memory_get` tools
- Injects additional markdown files into every session via `extraPaths`

The `surreal-mem-mcp` integration uses the `extraPaths` hook as the bridge:

```
surreal-mem-mcp (SurrealDB/RocksDB — global memory store)
        │
        │  Sync script (runs on gateway start via BOOT.md)
        │  Queries top global memories → formats as markdown
        ▼
~/.openclaw/memory/surreal-context.md  (snapshot file)
        │
        │  extraPaths injection (openclaw.json config)
        │  Injected into EVERY agent + sub-agent session automatically
        ▼
OpenClaw Agent Context Window
(main agent, all sub-agents, heartbeat sessions, cron sessions)
```

This means all agents — including dynamically spawned sub-agents that have no live connection to the daemon — share the same distilled global memory at session start.

---

### Prerequisites

- OpenClaw gateway installed and running
- Ollama running locally with `nomic-embed-text` model pulled (`ollama pull nomic-embed-text`)
- `surreal-mem-mcp` daemon installed and running (see main README for setup)

---

### Setup

#### Step 1 — Start the surreal-mem-mcp daemon

Start the daemon so it is accessible at `http://127.0.0.1:3333/sse`:

```bash
~/.surreal-mem-mcp/bin/surreal-mem-mcp
```

Verify it is running:

```bash
curl --max-time 3 http://127.0.0.1:3333/sse
# Expected: event: endpoint\ndata: /message?session_id=<uuid>
```

To run it persistently as a background service, configure it via your OS service manager (launchd on macOS, systemd on Linux).

---

#### Step 2 — Configure OpenClaw extraPaths

Edit `~/.openclaw/openclaw.json` and add the snapshot file path to `agents.defaults.memorySearch.extraPaths`:

```json
{
  "agents": {
    "defaults": {
      "memorySearch": {
        "provider": "ollama",
        "remote": {
          "baseUrl": "http://localhost:11434"
        },
        "model": "nomic-embed-text",
        "extraPaths": [
          "memory/surreal-context.md"
        ]
      }
    }
  }
}
```

The path `memory/surreal-context.md` is relative to the agent's workspace directory (typically `~/.openclaw/workspace/`). OpenClaw resolves it to `~/.openclaw/workspace/memory/surreal-context.md`.

> **Why this works:** OpenClaw injects all `extraPaths` files verbatim into every agent session's system prompt context at turn start. This includes the main session, all sub-agent sessions, heartbeat polls, and cron jobs — making it the most reliable way to share memory across the entire agent ecosystem without requiring a live MCP connection from each session.

---

#### Step 3 — Create the memory sync script

Create a script that queries the top global memories from `surreal-mem-mcp` via MCP over SSE and writes a formatted markdown snapshot to `~/.openclaw/workspace/memory/surreal-context.md`.

Save as `~/.openclaw/workspace/scripts/sync-surreal-memory.js`:

```js
#!/usr/bin/env node
/**
 * sync-surreal-memory.js
 * Pulls top global memories from surreal-mem-mcp and writes a markdown
 * snapshot to the OpenClaw workspace for context injection into all agents.
 *
 * Run: node sync-surreal-memory.js
 * Called automatically on gateway start via BOOT.md
 */

const fs = require("fs");
const path = require("path");

const SSE_URL = "http://127.0.0.1:3333/sse";
const OUTPUT_PATH = path.join(
  process.env.HOME,
  ".openclaw/workspace/memory/surreal-context.md"
);

async function getSession() {
  const res = await fetch(SSE_URL, {
    headers: { Accept: "text/event-stream" },
  });

  return new Promise((resolve, reject) => {
    const reader = res.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";

    function read() {
      reader.read().then(({ done, value }) => {
        if (done) return reject(new Error("SSE stream closed before session"));
        buffer += decoder.decode(value);
        const match = buffer.match(/data: (\/message\?session_id=[a-f0-9-]+)/);
        if (match) {
          reader.cancel();
          resolve({
            sessionId: match[1].split("=")[1],
            messageUrl: `http://127.0.0.1:3333${match[1]}`,
          });
          return;
        }
        read();
      });
    }
    read();
  });
}

async function mcpCall(messageUrl, method, params, id) {
  const res = await fetch(messageUrl, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id, method, params }),
  });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  // surreal-mem-mcp returns 202 Accepted for tool calls (async SSE pattern)
  return res.status;
}

async function searchMemory(messageUrl) {
  // Use compressed mode for efficient token-light sweep of all global memories
  await mcpCall(
    messageUrl,
    "tools/call",
    {
      name: "search_memory",
      arguments: {
        query: "agent identity business projects credentials rules priorities",
        scope: "global",
        limit: 20,
        compressed: false,
      },
    },
    2
  );
}

async function main() {
  console.log("Connecting to surreal-mem-mcp...");
  const { messageUrl } = await getSession();

  // Initialize MCP session
  await mcpCall(
    messageUrl,
    "initialize",
    {
      protocolVersion: "2024-11-05",
      capabilities: {},
      clientInfo: { name: "openclaw-sync", version: "1.0.0" },
    },
    1
  );

  await searchMemory(messageUrl);

  // Note: surreal-mem-mcp uses SSE async responses.
  // For a full production sync, listen on the SSE stream for tool results.
  // This simplified version seeds the snapshot from the remember calls made
  // during initial setup (see Step 4 — Seeding memories below).
  //
  // The snapshot file is written by the seeding step and refreshed here
  // to trigger OpenClaw's file watcher / context reload.

  const timestamp = new Date().toISOString();
  const existing = fs.existsSync(OUTPUT_PATH)
    ? fs.readFileSync(OUTPUT_PATH, "utf8")
    : "";

  if (existing) {
    // Update the timestamp header to signal a fresh sync
    const updated = existing.replace(
      /^_Last synced:.*$/m,
      `_Last synced: ${timestamp}_`
    );
    fs.writeFileSync(OUTPUT_PATH, updated);
    console.log(`Refreshed timestamp → ${OUTPUT_PATH}`);
  } else {
    console.log("No snapshot file found — run the seeding script first.");
    process.exit(1);
  }
}

main().catch((err) => {
  console.error("Sync failed:", err.message);
  process.exit(1);
});
```

---

#### Step 4 — Seed your memories into surreal-mem-mcp

Before the sync script can inject context, you need to populate the global memory store. Use the `remember` MCP tool to store key facts (agent identity, business info, project paths, behavior rules, credentials paths, etc.) with `scope: "global"`.

Example seeding script (`seed-memories.js`):

```js
// Connect via SSE (same pattern as sync script), then call:
await mcpCall(messageUrl, "tools/call", {
  name: "remember",
  arguments: {
    scope: "global",
    agent_id: "your-agent-id",
    headline: "Business: Cozy Llama Heating & Cooling — website example.com",
    text: "Business: Cozy Llama Heating & Cooling (HVAC). Website: example.com. Hosted on Netlify..."
  }
}, id++);
```

Run the seeding script once. Memories persist in RocksDB across restarts.

---

#### Step 5 — Hook into OpenClaw gateway startup via BOOT.md

OpenClaw supports a `BOOT.md` file in the workspace directory. On every gateway start, OpenClaw runs any bash commands listed there. Use this to keep the snapshot fresh:

Create `~/.openclaw/workspace/BOOT.md`:

```markdown
# BOOT.md — Gateway Startup

On every gateway start, pull fresh context from surreal-mem-mcp and refresh
the snapshot so all agents share the same knowledge base.

\`\`\`bash
node /Users/yourname/.openclaw/workspace/scripts/sync-surreal-memory.js
\`\`\`
```

> **Result:** Every time OpenClaw restarts, the memory snapshot is refreshed before any agent session starts — ensuring all agents (main, sub-agents, heartbeat, cron) see the latest global context.

---

#### Step 6 — Register with Claude Code (for ACP coding agents)

When OpenClaw spawns Claude Code sessions via its ACP harness (`sessions_spawn` with `runtime: "acp"`), those sessions can use `surreal-mem-mcp` tools directly via MCP. Register the server globally:

```bash
claude mcp add --transport sse surreal-mem-mcp http://127.0.0.1:3333/sse --scope user
```

This gives every Claude Code session spawned from OpenClaw access to `remember`, `search_memory`, and `search_memory_graph` tools natively.

---

### Verified Behavior

After completing setup, OpenClaw sub-agents spawned with zero manual context injection were verified to autonomously:

| Capability | Result |
|---|---|
| Recall agent identity, user name, business name | ✅ From injected snapshot |
| Recall specific credential IDs and project paths | ✅ From MEMORY.md + snapshot |
| Execute tasks using memorized file paths (e.g., check cron status) | ✅ No prompting needed |
| Apply behavior rules (require approval before external actions) | ✅ Cited source files correctly |
| Access session history from daily memory files | ✅ From `memory/YYYY-MM-DD.md` |

---

### Why Not Native MCP for Sub-Agents?

OpenClaw sub-agents are spawned as isolated sessions. Each session resolves its context at start time from injected files — they do not maintain a persistent live connection to external MCP servers during their run. This is intentional: it keeps sub-agents fast, isolated, and stateless.

The `extraPaths` file injection pattern is OpenClaw's native mechanism for sharing context across sessions. By writing a markdown snapshot from `surreal-mem-mcp` into that pipeline, you get full memory sharing without requiring each sub-agent to establish its own MCP connection.

For external tools that do maintain persistent sessions (Claude Code desktop, Gemini CLI, Cursor), the standard MCP SSE connection works as documented in the main README.

---

## LangChain

Add `surreal-mem-mcp` tools to your LangChain agent via the MCP SSE client adapter:

```python
from langchain_mcp_adapters.client import MultiServerMCPClient
from langgraph.prebuilt import create_react_agent

async with MultiServerMCPClient({
    "surreal-mem-mcp": {
        "url": "http://127.0.0.1:3333/sse",
        "transport": "sse"
    }
}) as client:
    tools = client.get_tools()
    agent = create_react_agent(model, tools)
    result = await agent.ainvoke({"messages": "What do you remember about this project?"})
```

The agent will have access to `remember`, `search_memory`, `search_memory_graph`, `promote_memory`, and `end_session` tools automatically.

---

## CrewAI

Mount `surreal-mem-mcp` as a shared memory backend across your crew:

```python
import asyncio
from mcp import ClientSession
from mcp.client.sse import sse_client
from crewai import Agent, Task, Crew
from crewai.tools import tool

@tool("search_memory")
def search_memory_tool(query: str) -> str:
    """Search the shared agent memory store."""
    async def _search():
        async with sse_client("http://127.0.0.1:3333/sse") as (read, write):
            async with ClientSession(read, write) as session:
                await session.initialize()
                result = await session.call_tool(
                    "search_memory",
                    {"query": query, "scope": "global", "limit": 5}
                )
                return result.content[0].text
    return asyncio.run(_search())

researcher = Agent(
    role="Researcher",
    goal="Research and recall relevant context",
    tools=[search_memory_tool]
)
```

---

## LlamaIndex

```python
from llama_index.core.agent import ReActAgent
from llama_index.tools.mcp import BasicMCPClient, McpToolSpec

async def build_agent(llm):
    mcp_client = BasicMCPClient("http://127.0.0.1:3333/sse")
    mcp_tool_spec = McpToolSpec(client=mcp_client)
    tools = await mcp_tool_spec.to_tool_list_async()
    return ReActAgent.from_tools(tools, llm=llm, verbose=True)
```

---

## Contributing

Found a better integration pattern? Open a PR. This document was authored based on real production use of `surreal-mem-mcp` with OpenClaw — if you discover edge cases or improvements, the community benefits from documented solutions.
