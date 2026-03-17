use crate::embeddings::openai::OpenAiClient;
use crate::surreal_client::SurrealClient;
use serde_json::{Value, json};
use std::sync::Arc;

pub fn list_tools() -> Vec<Value> {
    vec![
        json!({
            "name": "remember",
            "description": "Store a memory with its embeddings in the Bayesian Graph memory store.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": { "type": "string" },
                    "scope": { "type": "string", "enum": ["global", "agent", "session"], "description": "The scope of this memory." },
                    "agent_id": { "type": "string", "description": "The ID of the agent (if applicable)." },
                    "session_id": { "type": "string", "description": "The ID of the current session (if applicable)." },
                    "metadata": { "type": "object" }
                },
                "required": ["text", "scope"]
            }
        }),
        json!({
            "name": "index_codebase",
            "description": "Index a local codebase using tree-sitter to parse AST structure (Functions, Classes, Imports) into the SurrealDB property graph context.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "The absolute path to the local directory containing the source code." }
                },
                "required": ["path"]
            }
        }),
        json!({
            "name": "search_memory",
            "description": "Search the memory store using Bayesian Fusion (70% Vector + 30% BM25) and Epistemic Uncertainty checks.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "scope": { "type": "string", "enum": ["global", "agent", "session"], "description": "The scope of the search." },
                    "agent_id": { "type": "string", "description": "The ID of the agent (if applicable)." },
                    "session_id": { "type": "string", "description": "The ID of the current session (if applicable)." },
                    "limit": { "type": "integer", "default": 5 }
                },
                "required": ["query", "scope"]
            }
        }),
        json!({
            "name": "search_memory_graph",
            "description": "Perform a deep multi-hop knowledge graph traversal starting from the most relevant Bayesian memory match.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "scope": { "type": "string", "enum": ["global", "agent", "session"], "description": "The scope of the search." },
                    "agent_id": { "type": "string", "description": "The ID of the agent (if applicable)." },
                    "session_id": { "type": "string", "description": "The ID of the current session (if applicable)." },
                    "max_depth": { "type": "integer", "default": 5, "description": "The maximum number of graph edges to traverse." }
                },
                "required": ["query", "scope"]
            }
        }),
        json!({
            "name": "update_behavioral_rules",
            "description": "Append or rewrite the learned user preferences in the dynamic MEMORY.md file.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "content": { "type": "string", "description": "The new Markdown content or rules to append/write." },
                    "overwrite": { "type": "boolean", "description": "If true, overwrites MEMORY.md completely. If false, appends." }
                },
                "required": ["content"]
            }
        }),
        json!({
            "name": "end_session",
            "description": "Prune ephemeral session memories to prevent context bloat.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "The ID of the session to end and delete." }
                },
                "required": ["session_id"]
            }
        }),
        json!({
            "name": "promote_memory",
            "description": "If you learn a highly valuable, reusable fact during a session that you believe will be useful in future conversations, use this tool to promote the memory_id to the global or agent scope. This preserves its Bayesian graph edges and access frequency.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "memory_id": { "type": "string", "description": "The ID of the memory to promote (e.g. 'memory:xyz')." },
                    "target_scope": { "type": "string", "enum": ["global", "agent"], "description": "The target scope to promote this memory into." }
                },
                "required": ["memory_id", "target_scope"]
            }
        }),
    ]
}

pub async fn call_tool(params: Value, db: Arc<SurrealClient>) -> Value {
    let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    let openai_client = OpenAiClient::new();

    match name {
        "remember" => {
            let text = args.get("text").and_then(|t| t.as_str()).unwrap_or("");
            let scope = args
                .get("scope")
                .and_then(|s| s.as_str())
                .unwrap_or("global");
            let agent_id = args.get("agent_id").and_then(|a| a.as_str());
            let session_id = args.get("session_id").and_then(|s| s.as_str());
            let metadata = args.get("metadata").cloned().unwrap_or(json!({}));

            let safe_text = crate::security::redactor::redact_sensitive_data(text);
            let emb = openai_client.get_embedding(&safe_text).await.ok();

            match db
                .remember(&safe_text, emb, metadata, scope, agent_id, session_id)
                .await
            {
                Ok(id) => json!({
                    "content": [
                        { "type": "text", "text": format!("Memory saved with ID: {}", id) }
                    ]
                }),
                Err(e) => json!({
                    "isError": true,
                    "content": [
                        { "type": "text", "text": format!("Error: {}", e) }
                    ]
                }),
            }
        }
        "index_codebase" => {
            let path = args.get("path").and_then(|p| p.as_str()).unwrap_or("");
            match crate::ast::indexer::index_local_codebase(path.to_string(), db.db()).await {
                Ok(msg) => json!({
                    "content": [
                        { "type": "text", "text": msg }
                    ]
                }),
                Err(e) => json!({
                    "isError": true,
                    "content": [
                        { "type": "text", "text": format!("Error indexing codebase: {}", e) }
                    ]
                }),
            }
        }
        "search_memory" => {
            let query = args.get("query").and_then(|q| q.as_str()).unwrap_or("");
            let scope = args
                .get("scope")
                .and_then(|s| s.as_str())
                .unwrap_or("global");
            let agent_id = args.get("agent_id").and_then(|a| a.as_str());
            let session_id = args.get("session_id").and_then(|s| s.as_str());
            let limit = args.get("limit").and_then(|l| l.as_u64()).unwrap_or(5) as usize;

            let emb = openai_client.get_embedding(query).await.ok();

            match db
                .bayesian_search(query, emb, limit, scope, agent_id, session_id)
                .await
            {
                Ok(results) => {
                    json!({
                        "content": [
                            { "type": "text", "text": serde_json::to_string_pretty(&results).unwrap_or_default() }
                        ]
                    })
                }
                Err(e) => json!({
                    "isError": true,
                    "content": [
                        { "type": "text", "text": format!("Error: {}", e) }
                    ]
                }),
            }
        }
        "search_memory_graph" => {
            let query = args.get("query").and_then(|q| q.as_str()).unwrap_or("");
            let scope = args
                .get("scope")
                .and_then(|s| s.as_str())
                .unwrap_or("global");
            let agent_id = args.get("agent_id").and_then(|a| a.as_str());
            let session_id = args.get("session_id").and_then(|s| s.as_str());
            let max_depth = args.get("max_depth").and_then(|d| d.as_u64()).unwrap_or(5) as usize;

            let emb = openai_client.get_embedding(query).await.ok();

            match db
                .bayesian_graph_search(query, emb, max_depth, scope, agent_id, session_id)
                .await
            {
                Ok(results) => {
                    let mut output = format!(
                        "Traversed graph and found {} interconnected memory nodes:\n",
                        results.len()
                    );
                    for (idx, node) in results.iter().enumerate() {
                        if let Some(content) = node.get("text").and_then(|t| t.as_str()) {
                            output.push_str(&format!("[{}] {}\n", idx + 1, content));
                        }
                    }
                    if results.is_empty() {
                        output = "No memory graph found for that query.".to_string();
                    }
                    json!({
                        "content": [
                            { "type": "text", "text": output }
                        ]
                    })
                }
                Err(e) => json!({
                    "isError": true,
                    "content": [
                        { "type": "text", "text": format!("Error traversing graph: {}", e) }
                    ]
                }),
            }
        }
        "end_session" => {
            let session_id = args
                .get("session_id")
                .and_then(|s| s.as_str())
                .unwrap_or("");
            match db.end_session(session_id).await {
                Ok(_) => json!({
                    "content": [
                        { "type": "text", "text": format!("Successfully deleted ephemeral context for session: {}", session_id) }
                    ]
                }),
                Err(e) => json!({
                    "isError": true,
                    "content": [
                        { "type": "text", "text": format!("Error ending session: {}", e) }
                    ]
                }),
            }
        }
        "promote_memory" => {
            let memory_id = args.get("memory_id").and_then(|m| m.as_str()).unwrap_or("");
            let target_scope = args
                .get("target_scope")
                .and_then(|t| t.as_str())
                .unwrap_or("global");

            match db.promote_memory(memory_id, target_scope).await {
                Ok(_) => json!({
                    "content": [
                        { "type": "text", "text": format!("Successfully promoted memory {} to scope: {}", memory_id, target_scope) }
                    ]
                }),
                Err(e) => json!({
                    "isError": true,
                    "content": [
                        { "type": "text", "text": format!("Error promoting memory: {}", e) }
                    ]
                }),
            }
        }
        "update_behavioral_rules" => {
            let content = args.get("content").and_then(|c| c.as_str()).unwrap_or("");
            let overwrite = args
                .get("overwrite")
                .and_then(|o| o.as_bool())
                .unwrap_or(false);

            let rules_dir = crate::resources::get_rules_dir();
            let memory_path = rules_dir.join("MEMORY.md");

            let result = if overwrite {
                std::fs::write(&memory_path, content)
            } else {
                use std::fs::OpenOptions;
                use std::io::Write;
                let file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&memory_path);
                match file {
                    Ok(mut f) => writeln!(f, "\n{}", content),
                    Err(e) => Err(e),
                }
            };

            match result {
                Ok(_) => json!({
                    "content": [
                        { "type": "text", "text": format!("Successfully updated working memory at {:?}", memory_path) }
                    ]
                }),
                Err(e) => json!({
                    "isError": true,
                    "content": [
                        { "type": "text", "text": format!("Error updating memory: {}", e) }
                    ]
                }),
            }
        }
        _ => json!({
            "isError": true,
            "content": [
                { "type": "text", "text": format!("Unknown tool: {}", name) }
            ]
        }),
    }
}
