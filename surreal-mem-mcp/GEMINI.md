# Gemini CLI Playbook

Use this configuration to connect the Gemini CLI to the Surreal-Mem-MCP server.

## Installation
1. Download the latest compiled binary from the GitHub releases page for your OS.
2. Add the path to the binary in your MCP config.

## Example Config
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

When interacting with Gemini, you can explicitly ask it to query `surreal-mem-mcp` to search for contexts, architectural decisions, and previous history, while relying on the Mathematical Bayesian Posterior scoring to prevent hallucination.
