use crate::surreal_client::SurrealClient;
use serde_json::{Value, json};
use std::sync::Arc;

pub fn list_tools() -> Vec<Value> {
    vec![
        json!({
            "name": "remember",
            "description": "Store a memory with its embeddings in the Bayesian Graph memory store. For lossless context retrieval, provide a `headline` — a 1-2 sentence compressed summary of the core insight. The agent uses full `text` for precise recall and `headline` for efficient broad context sweeps.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": "The full, complete memory text to store." },
                    "headline": { "type": "string", "description": "A 1-2 sentence compressed summary of the core insight. Used for lossless context retrieval without flooding the context window. If omitted, the full text is used as fallback." },
                    "scope": { "type": "string", "enum": ["global", "agent", "session"], "description": "The scope of this memory." },
                    "agent_id": { "type": "string", "description": "The ID of the agent (if applicable)." },
                    "session_id": { "type": "string", "description": "The ID of the current session (if applicable)." },
                    "author_agent_id": { "type": "string", "description": "The specific sub-agent that authored this memory. Injected automatically from X-Agent-Id header if omitted. Used for swarm debugging and write attribution." },
                    "ttl_days": { "type": "integer", "description": "Optional TTL in days. After this many days the memory is passively evicted. If omitted, the memory never expires (unless it is session-scoped, which always expires after 24h)." },
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
            "description": "Search the memory store using Bayesian Fusion (70% Vector + 30% BM25) and Epistemic Uncertainty checks. Use `compressed=true` for efficient broad context sweeps that return only the headline summaries (lossless context mode — 5-10x lower token cost). Use `compressed=false` (default) for precise full-fidelity recall.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "scope": { "type": "string", "enum": ["global", "agent", "session"], "description": "The scope of the search." },
                    "agent_id": { "type": "string", "description": "The ID of the agent (if applicable)." },
                    "session_id": { "type": "string", "description": "The ID of the current session (if applicable)." },
                    "limit": { "type": "integer", "default": 5 },
                    "compressed": { "type": "boolean", "default": false, "description": "If true, returns headline summaries instead of full memory text. Use for broad context sweeps to minimize token consumption." }
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
        json!({
            "name": "check_index_status",
            "description": "Check whether a local codebase path has already been indexed. Call this before `index_codebase` to prevent duplicate concurrent indexing runs in a multi-agent swarm. Returns { indexed, file_count, func_count, last_indexed_at }.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "The absolute path to check index status for." }
                },
                "required": ["path"]
            }
        }),
    ]
}

pub async fn call_tool(params: Value, db: Arc<SurrealClient>) -> Value {
    let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or(json!({}));

    match name {
        "remember" => {
            let text = args.get("text").and_then(|t| t.as_str()).unwrap_or("");
            let headline = args.get("headline").and_then(|h| h.as_str());
            let scope = args
                .get("scope")
                .and_then(|s| s.as_str())
                .unwrap_or("global");
            let agent_id = args.get("agent_id").and_then(|a| a.as_str());
            let session_id = args.get("session_id").and_then(|s| s.as_str());
            let author_agent_id = args.get("author_agent_id").and_then(|a| a.as_str());
            let ttl_days = args.get("ttl_days").and_then(|t| t.as_u64()).map(|v| v as u32);
            let metadata = args.get("metadata").cloned().unwrap_or(json!({}));

            let safe_text = crate::security::redactor::redact_sensitive_data(text);
            let safe_headline = headline.map(|h| crate::security::redactor::redact_sensitive_data(h));
            let safe_headline_ref = safe_headline.as_deref();

            // Embed the full text for precise vector search
            let emb = db.get_embedding(&safe_text).await;

            // Embed the headline separately so compressed searches can still
            // use vector similarity against the summary representation.
            let headline_emb = if let Some(hl) = safe_headline_ref {
                if hl.len() > 10 {
                    db.get_embedding(hl).await
                } else {
                    None
                }
            } else { None };

            match db
                .remember(&safe_text, safe_headline_ref, emb, headline_emb, metadata, scope, agent_id, session_id, author_agent_id, ttl_days)
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
            let compressed = args.get("compressed").and_then(|c| c.as_bool()).unwrap_or(false);

            let emb = db.get_embedding(query).await;

            match db
                .bayesian_search(query, emb, limit, scope, agent_id, session_id, compressed)
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

            let emb = db.get_embedding(query).await;

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
        "check_index_status" => {
            let path = args.get("path").and_then(|p| p.as_str()).unwrap_or("");
            match db.db()
                .query("SELECT count() AS file_count, math::max(indexed_at) AS last_indexed_at FROM file WHERE path CONTAINS $path_prefix GROUP BY all")
                .bind(("path_prefix", path))
                .await
            {
                Ok(mut res) => {
                    let rows: Vec<Value> = res.take(0).unwrap_or_default();
                    let (file_count, last_indexed_at) = if let Some(row) = rows.first() {
                        (
                            row.get("file_count").and_then(|v| v.as_u64()).unwrap_or(0),
                            row.get("last_indexed_at").cloned().unwrap_or(json!(null)),
                        )
                    } else {
                        (0, json!(null))
                    };

                    let func_rows: Vec<Value> = db.db()
                        .query("SELECT count() AS func_count FROM func WHERE path CONTAINS $path_prefix GROUP BY all")
                        .bind(("path_prefix", path))
                        .await
                        .ok()
                        .and_then(|mut r| r.take(0).ok())
                        .unwrap_or_default();
                    let func_count = func_rows.first()
                        .and_then(|r| r.get("func_count"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);

                    json!({
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string_pretty(&json!({
                                "indexed": file_count > 0,
                                "file_count": file_count,
                                "func_count": func_count,
                                "last_indexed_at": last_indexed_at
                            })).unwrap_or_default()
                        }]
                    })
                }
                Err(e) => json!({
                    "isError": true,
                    "content": [{ "type": "text", "text": format!("Error checking index status: {}", e) }]
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
