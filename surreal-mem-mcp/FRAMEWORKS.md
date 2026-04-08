# Multi-Agent Framework Integration

The `surreal-mem-mcp` daemon is specifically designed for interoperability via the Anthropics **Model Context Protocol (MCP)**. Because MCP has become the industry standard "USB-C port for AI," you don't need custom memory plugins for every agent framework. 

Instead, you attach the daemon as a standard **MCP Server**, and your orchestrator frameworks automatically map the Graph-RAG memory tools (`remember`, `search_memory`, `end_session`) directly into their native Agent workflows.

Here is how you wire up `surreal-mem-mcp` to popular open-source autonomous agent frameworks:

---

## 1. Openclaw & Nemoclaw

**Openclaw** and **Nemoclaw** implement MCP natively in their Rust cores. `surreal-mem-mcp` integrates as a standard MCP provider — no extra pip installs required.

> **Transport:** `surreal-mem-mcp` runs HTTP/SSE only. STDIO is not supported. If you accidentally point Openclaw at the binary expecting STDIO, issue a `GET /` to confirm the transport — the server returns a JSON info payload with the correct SSE endpoint.

### Step 1 — Register the Server

Point Openclaw at the running HTTP/SSE daemon:

```yaml
# openclaw_config.yaml
mcp_servers:
  surreal-memory:
    type: sse
    url: "http://127.0.0.1:24555/sse"
```

Start the daemon before launching your Openclaw swarm:

```bash
./surreal-mem-mcp  # or the full binary path
```

### Step 2 — Inject Agent Identity via Headers (Recommended)

The most reliable architecture for swarm deployments is **orchestrator-level header injection**. Instead of relying on each sub-agent LLM to remember its `agent_id`/`session_id`, your Openclaw middleware layer sets `X-Agent-Id` and `X-Session-Id` HTTP headers on every `/message` request. The daemon injects these as fallbacks into any tool call that omits them — explicit tool args always win.

Here is a minimal Rust HTTP middleware snippet for an Openclaw Hand wrapper:

```rust
// In your Openclaw Hand implementation, wrap the MCP client calls:
fn inject_identity_headers(req: &mut http::Request<Body>, hand: &HandContext) {
    req.headers_mut().insert(
        "x-agent-id",
        hand.agent_id.parse().unwrap(),
    );
    req.headers_mut().insert(
        "x-session-id",
        hand.task_id.parse().unwrap(),
    );
}
```

With this in place, every `remember`, `search_memory`, `search_memory_graph`, and `promote_memory` call is automatically scoped to the correct agent and task — no LLM cooperation required.

### Step 3 — Terminate Sessions via REST (Lifecycle Hook)

When an Openclaw task completes, call the session terminate endpoint directly from your orchestrator. This eagerly prunes ephemeral session memories without requiring the LLM to call `end_session`:

```rust
// In your Openclaw task lifecycle manager, on task completion:
let _ = reqwest::Client::new()
    .delete(format!("http://127.0.0.1:24555/session/{}", task_id))
    .send()
    .await;
```

```bash
# Or directly from a shell lifecycle hook:
curl -X DELETE http://127.0.0.1:24555/session/<task_id>
```

### Step 4 — Pre-flight Index Check for Codebase Indexing

In a multi-agent Openclaw swarm, multiple Hands may trigger `index_codebase` for the same repo concurrently. Use `check_index_status` first to avoid duplicate work:

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
    await client.connect_sse("surreal_memory", "http://localhost:24555/sse")
    
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
    await client.connect_sse("memory_node", "http://localhost:24555/sse")
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
mcp_client = MCPClient(url="http://localhost:24555/sse")
tools = mcp_client.get_tools()

# LlamaIndex agents can now autonomously decide when to query the graph vs when to insert
```

## Abstracting Context Variables
For **ALL** frameworks, the most robust enterprise architecture involves utilizing a *Middleware Interceptor*. Instead of relying on the LLM to successfully remember its `agent_id` or `session_id`, you can intercept the JSON-RPC execution payloads inside LangChain/CrewAI/Openclaw *before* they are sent to `surreal-mem-mcp`, forcefully overwriting the `scope` properties natively in the host language.
