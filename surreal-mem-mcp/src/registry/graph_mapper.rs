use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use serde_json::Value;
use surrealdb::Surreal;
use surrealdb::engine::any::Any;
use crate::embeddings::local::LocalEmbedder;

fn deterministic_hash(data: &str) -> String {
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    format!("{}", hasher.finish())
}

/// Build graph edges between skills and tools.
///
/// 1. **Explicit mapping**: For each skill with `suggested_tools`, look up
///    matching `tool` records by name and create `requires` edges with
///    confidence 1.0.
///
/// 2. **Semantic fallback**: For all skills, compute cosine similarity between
///    the skill's embedding and each tool's embedding. Create edges for pairs
///    with similarity > 0.75 (if no explicit edge already exists).
pub async fn build_skill_tool_graph(
    db: Arc<Surreal<Any>>,
    embedder: Arc<LocalEmbedder>,
) -> Result<String, String> {
    let mut edge_count = 0;

    // 1. Explicit edges from suggested_tools
    let mut skill_res = db.query("SELECT id, suggested_tools, embedding FROM skill")
        .await
        .map_err(|e| e.to_string())?;
    let skills: Vec<Value> = skill_res.take(0).unwrap_or_default();

    for skill in &skills {
        let skill_id_raw = match skill.get("id") {
            Some(id) => id,
            None => continue,
        };

        let suggested_tools = skill.get("suggested_tools")
            .and_then(|st| st.as_array())
            .cloned()
            .unwrap_or_default();

        for suggested in &suggested_tools {
            let tool_name = match suggested.get("name").and_then(|n| n.as_str()) {
                Some(n) => n,
                None => continue,
            };

            // Look up the tool by name
            let mut tool_res = db.query(
                "SELECT id FROM tool WHERE name = $tool_name LIMIT 1"
            )
            .bind(("tool_name", tool_name.to_string()))
            .await
            .map_err(|e| e.to_string())?;

            let tool_rows: Vec<Value> = tool_res.take(0).unwrap_or_default();
            if let Some(tool_row) = tool_rows.first() {
                if let Some(tool_id) = tool_row.get("id") {
                    // Build deterministic edge ID
                    let skill_id_str = serde_json::to_string(skill_id_raw).unwrap_or_default();
                    let tool_id_str = serde_json::to_string(tool_id).unwrap_or_default();
                    let edge_hash = deterministic_hash(&format!("{}_{}", skill_id_str, tool_id_str));

                    let _ = db.query(&format!(
                        "UPSERT requires:⟨{}⟩ CONTENT {{ in: $skill_id, out: $tool_id, source: 'explicit', confidence: 1.0 }}",
                        edge_hash
                    ))
                    .bind(("skill_id", skill_id_raw.clone()))
                    .bind(("tool_id", tool_id.clone()))
                    .await
                    .map_err(|e| e.to_string())?;

                    edge_count += 1;
                }
            }
        }
    }

    // 2. Semantic fallback: cosine similarity between skill and tool embeddings
    let mut tool_res = db.query("SELECT id, name, embedding FROM tool WHERE embedding IS NOT NONE")
        .await
        .map_err(|e| e.to_string())?;
    let tools: Vec<Value> = tool_res.take(0).unwrap_or_default();

    for skill in &skills {
        let skill_id_raw = match skill.get("id") {
            Some(id) => id,
            None => continue,
        };

        let skill_emb = match skill.get("embedding").and_then(|e| e.as_array()) {
            Some(arr) => arr.iter().filter_map(|v| v.as_f64().map(|f| f as f32)).collect::<Vec<f32>>(),
            None => continue,
        };

        if skill_emb.is_empty() {
            continue;
        }

        for tool in &tools {
            let tool_id = match tool.get("id") {
                Some(id) => id,
                None => continue,
            };

            let tool_emb = match tool.get("embedding").and_then(|e| e.as_array()) {
                Some(arr) => arr.iter().filter_map(|v| v.as_f64().map(|f| f as f32)).collect::<Vec<f32>>(),
                None => continue,
            };

            if tool_emb.is_empty() || tool_emb.len() != skill_emb.len() {
                continue;
            }

            let similarity = cosine_similarity(&skill_emb, &tool_emb);

            if similarity > 0.75 {
                let skill_id_str = serde_json::to_string(skill_id_raw).unwrap_or_default();
                let tool_id_str = serde_json::to_string(tool_id).unwrap_or_default();
                let edge_hash = deterministic_hash(&format!("{}_{}", skill_id_str, tool_id_str));

                // Only create if no explicit edge already exists (don't downgrade explicit to semantic)
                let mut check = db.query(&format!(
                    "SELECT source FROM requires:⟨{}⟩", edge_hash
                ))
                .await
                .map_err(|e| e.to_string())?;

                let existing: Vec<Value> = check.take(0).unwrap_or_default();
                let already_explicit = existing.first()
                    .and_then(|v| v.get("source"))
                    .and_then(|s| s.as_str())
                    .map(|s| s == "explicit")
                    .unwrap_or(false);

                if !already_explicit {
                    let _ = db.query(&format!(
                        "UPSERT requires:⟨{}⟩ CONTENT {{ in: $skill_id, out: $tool_id, source: 'semantic', confidence: $sim }}",
                        edge_hash
                    ))
                    .bind(("skill_id", skill_id_raw.clone()))
                    .bind(("tool_id", tool_id.clone()))
                    .bind(("sim", similarity))
                    .await
                    .map_err(|e| e.to_string())?;

                    edge_count += 1;
                }
            }
        }
    }

    Ok(format!("Built {} skill→tool graph edges.", edge_count))
}

/// Compute cosine similarity between two vectors.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![1.0, 0.0, 1.0];
        let b = vec![1.0, 0.0, 1.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 0.001);
    }

    #[test]
    fn test_cosine_similarity_similar() {
        let a = vec![1.0, 0.8, 0.6];
        let b = vec![0.9, 0.7, 0.5];
        let sim = cosine_similarity(&a, &b);
        assert!(sim > 0.99); // Very similar vectors
    }
}
