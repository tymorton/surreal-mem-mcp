# Multi-Agent Framework Integration

The `surreal-mem-mcp` daemon is specifically designed for interoperability via the Anthropics **Model Context Protocol (MCP)**. Because MCP has become the industry standard "USB-C port for AI," you don't need custom memory plugins for every agent framework. 

Instead, you attach the daemon as a standard **MCP Server**, and your orchestrator frameworks automatically map the Graph-RAG memory tools (`remember`, `search_memory`, `end_session`) directly into their native Agent workflows.

Here is how you wire up `surreal-mem-mcp` to popular open-source autonomous agent frameworks:

---

## 1. Openclaw & Nemoclaw

**Openclaw** and **Nemoclaw** natively implement the Model Context Protocol directly in their Rust cores. Because they rely on autonomous "Hands" (capability modules) and robust inter-agent communication, they can inherently map MCP tools into their operations without requiring third-party pip installs.

### How to Integrate
In your Openclaw or Nemoclaw deployment configuration, you register `surreal-mem-mcp` directly as an MCP provider. Because these frameworks manage their own multi-agent orchestrations, you can map the `agent_id` parameter dynamically so each sub-agent maintains memory isolation.

```yaml
# Inside your Openclaw MCP configuration
mcp_servers:
  surreal-memory:
    command: "target/release/surreal-mem-mcp" # Or the executed binary path
    env:
      SURREAL_DB_PATH: "memory_store"
      PORT: "3000"
```
*Note: Depending on Openclaw's exact connector implementation, you can either trigger this via standard STDIO execution or by connecting directly to the running `http://localhost:3000/sse` HTTP/SSE stream.*

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
