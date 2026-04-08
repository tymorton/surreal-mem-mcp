use std::sync::Arc;
use serde_json::{Value, json};
use surrealdb::Surreal;
use surrealdb::engine::any::Any;
use crate::embeddings::local::LocalEmbedder;
use crate::registry::{skill_ingestor, tool_registry, graph_mapper};

// ── discover_capabilities ───────────────────────────────────────────────

/// Search across skills and tools using Bayesian vector + BM25 fusion.
/// Returns a concise summary — no full runbooks or schemas.
pub async fn discover_capabilities(
    db: Arc<Surreal<Any>>,
    embedder: Arc<LocalEmbedder>,
    intent: &str,
) -> Result<Value, String> {
    let query_emb = embedder.get_embedding(intent).await;

    // Search skills by description + intents embedding
    let skill_results = if let Some(ref emb) = query_emb {
        let mut res = db.query(r#"
            SELECT
                id,
                name,
                description,
                vector::similarity::cosine(embedding, $query_emb) AS confidence
            FROM skill
            ORDER BY confidence DESC
            LIMIT 5
        "#)
        .bind(("query_emb", emb.clone()))
        .await
        .map_err(|e| e.to_string())?;

        let rows: Vec<Value> = res.take(0).unwrap_or_default();
        rows
    } else {
        vec![]
    };

    // Search skill_chunks for deeper semantic match
    let chunk_skill_ids = if let Some(ref emb) = query_emb {
        let mut res = db.query(r#"
            SELECT
                skill_id,
                vector::similarity::cosine(embedding, $query_emb) AS score
            FROM skill_chunk
            ORDER BY score DESC
            LIMIT 5
        "#)
        .bind(("query_emb", emb.clone()))
        .await
        .map_err(|e| e.to_string())?;

        let rows: Vec<Value> = res.take(0).unwrap_or_default();
        // Deduplicate by skill_id
        let mut seen = std::collections::HashSet::new();
        rows.iter()
            .filter_map(|r| r.get("skill_id").and_then(|s| s.as_str()).map(|s| s.to_string()))
            .filter(|s| seen.insert(s.clone()))
            .collect::<Vec<String>>()
    } else {
        vec![]
    };

    // Merge chunk-discovered skills into results (dedup by id)
    let mut merged_skills = skill_results;
    for chunk_sid in &chunk_skill_ids {
        let already_listed = merged_skills.iter().any(|s| {
            let sid = serde_json::to_string(s.get("id").unwrap_or(&json!(null))).unwrap_or_default();
            sid.contains(chunk_sid)
        });
        if !already_listed {
            // Fetch this skill's summary
            let mut res = db.query(
                "SELECT id, name, description FROM skill WHERE id = type::record($sid) LIMIT 1"
            )
            .bind(("sid", chunk_sid.clone()))
            .await
            .map_err(|e| e.to_string())?;

            let rows: Vec<Value> = res.take(0).unwrap_or_default();
            if let Some(row) = rows.into_iter().next() {
                merged_skills.push(json!({
                    "id": row.get("id"),
                    "name": row.get("name"),
                    "description": row.get("description"),
                    "confidence": 0.0,
                    "source": "chunk_match"
                }));
            }
        }
    }

    // Search tools by description embedding
    let tool_results = if let Some(ref emb) = query_emb {
        let mut res = db.query(r#"
            SELECT
                id,
                name,
                description,
                status,
                `type`,
                vector::similarity::cosine(embedding, $query_emb) AS confidence
            FROM tool
            ORDER BY confidence DESC
            LIMIT 10
        "#)
        .bind(("query_emb", emb.clone()))
        .await
        .map_err(|e| e.to_string())?;

        let rows: Vec<Value> = match res.take(0) {
            Ok(r) => r,
            Err(e) => return Err(format!("Tool Query Error: {}", e)),
        };
        rows
    } else {
        vec![]
    };

    // Build concise response
    let skills_summary: Vec<Value> = merged_skills.iter().map(|s| {
        json!({
            "id": s.get("id"),
            "name": s.get("name").and_then(|n| n.as_str()).unwrap_or("unknown"),
            "description": s.get("description").and_then(|d| d.as_str()).unwrap_or(""),
            "confidence": s.get("confidence").and_then(|c| c.as_f64()).unwrap_or(0.0),
        })
    }).collect();

    let tools_summary: Vec<Value> = tool_results.iter().map(|t| {
        let mut base_desc = t.get("description").and_then(|d| d.as_str()).unwrap_or("").to_string();
        if t.get("status").and_then(|status| status.as_str()) == Some("degraded") {
            base_desc = format!("[⚠️ DEGRADED: This tool has failed its last 3 executions. Proceed with extreme caution or attempt to debug the underlying system.] {}", base_desc);
        }
        json!({
            "id": t.get("id"),
            "type": t.get("type").and_then(|ty| ty.as_str()).unwrap_or("unknown"),
            "name": t.get("name").and_then(|n| n.as_str()).unwrap_or("unknown"),
            "description": base_desc,
            "confidence": t.get("confidence").and_then(|c| c.as_f64()).unwrap_or(0.0),
        })
    }).collect();

    Ok(json!({
        "skills": skills_summary,
        "tools": tools_summary,
    }))
}

// ── get_skill_runbook ───────────────────────────────────────────────────

/// Fetch the full skill runbook + type-specific execution context for linked tools.
pub async fn get_skill_runbook(
    db: Arc<Surreal<Any>>,
    skill_id: &str,
) -> Result<Value, String> {
    // Fetch the skill record
    let mut skill_res = db.query(
        "SELECT name, description, intents, suggested_tools FROM type::record($skill_id)"
    )
    .bind(("skill_id", skill_id.to_string()))
    .await
    .map_err(|e| e.to_string())?;

    let skill_rows: Vec<Value> = skill_res.take(0).unwrap_or_default();
    let skill = skill_rows.first()
        .ok_or_else(|| format!("Skill not found: {}", skill_id))?;

    // The inbound skill_id may contain backticks or ⟨ ⟩; normalize to raw hash
    let raw_id = skill_id.replace("skill:", "").replace("`", "").replace("⟨", "").replace("⟩", "");
    let stored_sid = format!("skill:⟨{}⟩", raw_id);

    // Fetch all chunks ordered by index
    let mut chunk_res = db.query(
        "SELECT text, chunk_index FROM skill_chunk WHERE <string> skill_id = $stored_sid ORDER BY chunk_index ASC"
    )
    .bind(("stored_sid", stored_sid))
    .await
    .map_err(|e| e.to_string())?;

    let chunks: Vec<Value> = chunk_res.take(0).unwrap_or_default();
    let runbook_markdown: String = chunks.iter()
        .filter_map(|c| c.get("text").and_then(|t| t.as_str()))
        .collect::<Vec<&str>>()
        .join("\n");

    // Traverse requires edges to get linked tools with full execution context
    let mut tool_res = db.query(&format!(
        "SELECT ->requires->tool.* AS tools FROM type::record($skill_id)"
    ))
    .bind(("skill_id", skill_id.to_string()))
    .await
    .map_err(|e| e.to_string())?;

    let tool_traversal: Vec<Value> = tool_res.take(0).unwrap_or_default();

    // Extract tools from the graph traversal result
    let mut required_tools: Vec<Value> = Vec::new();

    if let Some(first) = tool_traversal.first() {
        if let Some(tools_arr) = first.get("tools").and_then(|t| t.as_array()) {
            for tool_group in tools_arr {
                // tools_arr may be nested arrays from graph traversal
                if let Some(inner) = tool_group.as_array() {
                    for tool in inner {
                        required_tools.push(build_tool_context(tool));
                    }
                } else if tool_group.is_object() {
                    required_tools.push(build_tool_context(tool_group));
                }
            }
        }
    }

    // Also fetch edge confidence scores
    let mut edge_res = db.query(
        "SELECT out, confidence, source FROM requires WHERE in = type::record($skill_id)"
    )
    .bind(("skill_id", skill_id.to_string()))
    .await
    .map_err(|e| e.to_string())?;

    let edges: Vec<Value> = edge_res.take(0).unwrap_or_default();
    let edge_map: std::collections::HashMap<String, (f64, String)> = edges.iter()
        .filter_map(|e| {
            let out_id = serde_json::to_string(e.get("out")?).ok()?;
            let conf = e.get("confidence").and_then(|c| c.as_f64()).unwrap_or(0.0);
            let source = e.get("source").and_then(|s| s.as_str()).unwrap_or("unknown").to_string();
            Some((out_id, (conf, source)))
        })
        .collect();

    // Enrich required_tools with confidence and source
    for tool in &mut required_tools {
        if let Some(tool_id) = tool.get("id") {
            let key = serde_json::to_string(tool_id).unwrap_or_default();
            if let Some((conf, source)) = edge_map.get(&key) {
                tool["confidence"] = json!(conf);
                tool["edge_source"] = json!(source);
            }
        }
    }

    Ok(json!({
        "skill": {
            "name": skill.get("name"),
            "description": skill.get("description"),
            "intents": skill.get("intents"),
        },
        "runbook_markdown": runbook_markdown,
        "required_tools": required_tools,
    }))
}

/// Build a concise tool context object for the LLM.
fn build_tool_context(tool: &Value) -> Value {
    json!({
        "id": tool.get("id"),
        "name": tool.get("name").and_then(|n| n.as_str()).unwrap_or("unknown"),
        "type": tool.get("type").and_then(|t| t.as_str()).unwrap_or("unknown"),
        "description": tool.get("description").and_then(|d| d.as_str()).unwrap_or(""),
        "execution_context": tool.get("execution_context").unwrap_or(&json!({})),
    })
}

// ── sync_registry ───────────────────────────────────────────────────────

/// Re-run the full ingestion pipeline: skills → tools → graph edges.
pub async fn sync_registry(
    db: Arc<Surreal<Any>>,
    embedder: Arc<LocalEmbedder>,
) -> Result<String, String> {
    let mut output = Vec::new();

    match skill_ingestor::ingest_skills(db.clone(), embedder.clone()).await {
        Ok(msg) => output.push(format!("Skills: {}", msg)),
        Err(e) => output.push(format!("Skills ERROR: {}", e)),
    }

    match tool_registry::sync_tool_registry(db.clone(), embedder.clone()).await {
        Ok(msg) => output.push(format!("Tools: {}", msg)),
        Err(e) => output.push(format!("Tools ERROR: {}", e)),
    }

    match graph_mapper::build_skill_tool_graph(db.clone(), embedder.clone()).await {
        Ok(msg) => output.push(format!("Graph: {}", msg)),
        Err(e) => output.push(format!("Graph ERROR: {}", e)),
    }

    Ok(output.join("\n"))
}

// ── remove_capability ───────────────────────────────────────────────────

/// Delete a skill (+ its chunks and edges) or a tool (+ its edges).
pub async fn remove_capability(
    db: Arc<Surreal<Any>>,
    capability_id: &str,
) -> Result<String, String> {
    if capability_id.starts_with("skill:") {
        // Delete chunks first
        let _ = db.query("DELETE skill_chunk WHERE skill_id = $sid")
            .bind(("sid", capability_id.to_string()))
            .await
            .map_err(|e| e.to_string())?;

        // Delete requires edges originating from this skill
        let _ = db.query("DELETE requires WHERE in = type::record($sid)")
            .bind(("sid", capability_id.to_string()))
            .await
            .map_err(|e| e.to_string())?;

        // Delete the skill itself
        let _ = db.query("DELETE type::record($sid)")
            .bind(("sid", capability_id.to_string()))
            .await
            .map_err(|e| e.to_string())?;

        Ok(format!("Removed skill and associated chunks/edges: {}", capability_id))
    } else if capability_id.starts_with("tool:") {
        // Delete requires edges pointing to this tool
        let _ = db.query("DELETE requires WHERE out = type::record($tid)")
            .bind(("tid", capability_id.to_string()))
            .await
            .map_err(|e| e.to_string())?;

        // Delete the tool
        let _ = db.query("DELETE type::record($tid)")
            .bind(("tid", capability_id.to_string()))
            .await
            .map_err(|e| e.to_string())?;

        Ok(format!("Removed tool and associated edges: {}", capability_id))
    } else {
        Err(format!("Invalid capability ID: {}. Must start with 'skill:' or 'tool:'.", capability_id))
    }
}
