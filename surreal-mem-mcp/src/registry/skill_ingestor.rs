use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use surrealdb::Surreal;
use surrealdb::engine::any::Any;
use crate::embeddings::local::LocalEmbedder;

// ── YAML Frontmatter Types ─────────────────────────────────────────────

#[derive(Debug, Deserialize, Default, Clone)]
pub struct SkillFrontmatter {
    pub name: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub intents: Vec<String>,
    #[serde(default)]
    pub suggested_tools: Vec<SuggestedTool>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SuggestedTool {
    pub name: String,
    #[serde(rename = "type", default = "default_tool_type")]
    pub tool_type: String,
    pub command: Option<String>,
}

fn default_tool_type() -> String {
    "mcp_server".to_string()
}

// ── Deterministic Hashing ──────────────────────────────────────────────

fn deterministic_hash(data: &str) -> String {
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    format!("{}", hasher.finish())
}

// ── Frontmatter Parser ─────────────────────────────────────────────────

/// Splits a markdown file into (frontmatter, body).
/// Frontmatter is delimited by `---` on its own line.
fn parse_frontmatter(content: &str) -> (SkillFrontmatter, String) {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return (SkillFrontmatter::default(), content.to_string());
    }

    // Find the closing `---`
    let after_first = &trimmed[3..];
    if let Some(end_idx) = after_first.find("\n---") {
        let yaml_block = &after_first[..end_idx].trim();
        let body_start = end_idx + 4; // skip "\n---"
        let body = after_first[body_start..].trim_start_matches('\n').to_string();

        match serde_yaml::from_str::<SkillFrontmatter>(yaml_block) {
            Ok(fm) => (fm, body),
            Err(e) => {
                println!("[skill_ingestor] YAML parse warning: {}", e);
                (SkillFrontmatter::default(), content.to_string())
            }
        }
    } else {
        (SkillFrontmatter::default(), content.to_string())
    }
}

// ── Markdown Chunker ────────────────────────────────────────────────────

/// Chunks text into ~2048-character windows with 128-character overlap.
fn chunk_markdown(text: &str, chunk_size: usize, overlap: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= chunk_size {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let end = (start + chunk_size).min(chars.len());
        let chunk: String = chars[start..end].iter().collect();
        chunks.push(chunk);
        if end >= chars.len() {
            break;
        }
        start = end.saturating_sub(overlap);
    }
    chunks
}

// ── Inbox/Archive Management ───────────────────────────────────────────

fn get_global_inbox_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".surreal-mem-mcp").join("inbox")
}

fn get_global_archive_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".surreal-mem-mcp").join("archive")
}

/// Ensures the archive directory has ignore files so coding agents skip it.
/// For local project archives: additionally .gitignore
fn ensure_archive_ignore_files(archive_dir: &Path, is_local: bool) {
    let _ = std::fs::create_dir_all(archive_dir);

    let mut ignore_files: Vec<&str> = vec![".cursorignore", ".aiderignore"];
    if is_local {
        ignore_files.push(".gitignore");
    }

    for filename in &ignore_files {
        let path = archive_dir.join(filename);
        if !path.exists() {
            let _ = std::fs::write(&path, "*\n");
        }
    }
}

/// Move a file from its current location to an archive directory.
fn move_to_archive(source: &Path, archive_dir: &Path) -> Result<PathBuf, String> {
    let _ = std::fs::create_dir_all(archive_dir);
    let filename = source.file_name()
        .ok_or_else(|| "No filename".to_string())?;
    let dest = archive_dir.join(filename);

    // If the destination already exists, add a timestamp suffix
    let final_dest = if dest.exists() {
        let stem = source.file_stem().unwrap_or_default().to_string_lossy();
        let ext = source.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
        let ts = chrono::Utc::now().format("%Y%m%d%H%M%S");
        archive_dir.join(format!("{}__{}{}", stem, ts, ext))
    } else {
        dest
    };

    std::fs::rename(source, &final_dest)
        .or_else(|_| {
            // Cross-device move: copy then delete
            std::fs::copy(source, &final_dest)
                .map_err(|e| format!("Copy failed: {}", e))?;
            std::fs::remove_file(source)
                .map_err(|e| format!("Remove after copy failed: {}", e))?;
            Ok::<(), String>(())
        })
        .map_err(|e: String| e)?;

    Ok(final_dest)
}

/// Determine the appropriate archive directory for a skill file.
/// Global files → ~/.surreal-mem-mcp/archive/
/// Local files  → .agents/archive/ or .mcp/archive/ (sibling of the skills dir)
fn get_archive_dir_for(skill_file: &SkillFile) -> PathBuf {
    if skill_file.is_local {
        // Derive from the file's parent: .agents/skills/ → .agents/archive/
        if let Some(parent) = skill_file.path.parent() {
            if let Some(grandparent) = parent.parent() {
                return grandparent.join("archive");
            }
        }
        let cwd = std::env::current_dir().unwrap_or_default();
        cwd.join(".agents").join("archive")
    } else {
        get_global_archive_dir()
    }
}

// ── Scan Directories ────────────────────────────────────────────────────

struct SkillFile {
    name: String,
    path: PathBuf,
    is_local: bool,
}

/// Scans global inbox and local project skills directories.
/// Local skills win on name collision.
fn collect_skill_files() -> Vec<SkillFile> {
    let mut skills_by_name: HashMap<String, SkillFile> = HashMap::new();

    // 1. Global inbox
    let inbox = get_global_inbox_dir();
    let _ = std::fs::create_dir_all(&inbox);
    if let Ok(entries) = std::fs::read_dir(&inbox) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("md") {
                let name = path.file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_lowercase()
                    .replace(' ', "-");
                skills_by_name.insert(name.clone(), SkillFile {
                    name,
                    path,
                    is_local: false,
                });
            }
        }
    }

    // 2. Local project directories (override on collision)
    let local_dirs = [".agents/skills", ".mcp/skills"];
    let cwd = std::env::current_dir().unwrap_or_default();
    for dir_name in &local_dirs {
        let local_dir = cwd.join(dir_name);
        if local_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&local_dir) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("md") {
                        let name = path.file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_lowercase()
                            .replace(' ', "-");
                        // Local wins over global
                        skills_by_name.insert(name.clone(), SkillFile {
                            name,
                            path,
                            is_local: true,
                        });
                    }
                }
            }
        }
    }

    skills_by_name.into_values().collect()
}

// ── Main Ingestion Function ─────────────────────────────────────────────

pub async fn ingest_skills(
    db: Arc<Surreal<Any>>,
    embedder: Arc<LocalEmbedder>,
) -> Result<String, String> {
    let skill_files = collect_skill_files();
    if skill_files.is_empty() {
        return Ok("No skill files found in inbox or local directories.".to_string());
    }

    let mut ingested_count = 0;
    let mut active_skill_ids: Vec<String> = Vec::new();

    for sf in &skill_files {
        let content = match std::fs::read_to_string(&sf.path) {
            Ok(c) => c,
            Err(e) => {
                println!("[skill_ingestor] Skipping {}: {}", sf.path.display(), e);
                continue;
            }
        };

        let (frontmatter, body) = parse_frontmatter(&content);

        // Derive name: YAML name > filename
        let skill_name = frontmatter.name.clone()
            .unwrap_or_else(|| sf.name.clone());

        let description = frontmatter.description.clone()
            .unwrap_or_else(|| {
                // First non-empty line of body as fallback description
                body.lines()
                    .find(|l| !l.trim().is_empty() && !l.starts_with('#'))
                    .unwrap_or("No description")
                    .to_string()
            });

        let skill_hash = deterministic_hash(&skill_name);
        active_skill_ids.push(format!("skill:⟨{}⟩", skill_hash));

        // Build concatenated embedding text: name + description + intents
        let embed_text = format!(
            "{} {} {}",
            skill_name,
            description,
            frontmatter.intents.join(" ")
        );
        let skill_embedding = embedder.get_embedding(&embed_text).await;

        let now = chrono::Utc::now().to_rfc3339();
        let suggested_tools_json = serde_json::to_value(&frontmatter.suggested_tools)
            .unwrap_or(json!([]));

        // Upsert skill record
        let mut skill_data = json!({
            "name": skill_name,
            "description": description,
            "intents": frontmatter.intents,
            "suggested_tools": suggested_tools_json,
            "source_path": sf.path.to_string_lossy().to_string(),
            "is_local": sf.is_local,
            "indexed_at": now,
        });
        if let Some(emb) = &skill_embedding {
            skill_data["embedding"] = json!(emb);
        }

        let _ = db.query(&format!(
            "UPSERT skill:⟨{}⟩ CONTENT $data", skill_hash
        ))
        .bind(("data", skill_data))
        .await
        .map_err(|e| e.to_string())?
        .check()
        .map_err(|e| e.to_string())?;

        // Delete old chunks then insert new ones
        let _ = db.query("DELETE skill_chunk WHERE skill_id = $sid")
            .bind(("sid", format!("skill:⟨{}⟩", skill_hash)))
            .await
            .map_err(|e| e.to_string())?;

        let chunks = chunk_markdown(&body, 2048, 128);
        for (idx, chunk_text) in chunks.iter().enumerate() {
            let chunk_hash = deterministic_hash(&format!("{}_{}", skill_name, idx));
            let chunk_emb = embedder.get_embedding(chunk_text).await;

            let mut chunk_data = json!({
                "skill_id": format!("skill:⟨{}⟩", skill_hash),
                "text": chunk_text,
                "chunk_index": idx,
            });
            if let Some(emb) = &chunk_emb {
                chunk_data["embedding"] = json!(emb);
            }

            let _ = db.query(&format!(
                "UPSERT skill_chunk:⟨{}⟩ CONTENT $data", chunk_hash
            ))
            .bind(("data", chunk_data))
            .await
            .map_err(|e| e.to_string())?
            .check()
            .map_err(|e| e.to_string())?;
        }

        // Archive: move file to the appropriate archive directory
        // Global files → ~/.surreal-mem-mcp/archive/
        // Local files  → .agents/archive/ (sibling of skills dir)
        let archive_dir = get_archive_dir_for(sf);
        ensure_archive_ignore_files(&archive_dir, sf.is_local);
        match move_to_archive(&sf.path, &archive_dir) {
            Ok(dest) => println!("[skill_ingestor] Archived: {} → {}", sf.path.display(), dest.display()),
            Err(e) => println!("[skill_ingestor] Archive warning: {}", e),
        }

        ingested_count += 1;
        println!("[skill_ingestor] Ingested skill: {} ({} chunks)", skill_name, chunks.len());
    }

    Ok(format!("Ingested {} skills successfully.", ingested_count))
}

/// Upsert a skill directly from LLM input (no filesystem needed).
/// This is the `learn_skill` tool backend.
pub async fn learn_skill(
    db: Arc<Surreal<Any>>,
    embedder: Arc<LocalEmbedder>,
    name: &str,
    description: &str,
    intents: Vec<String>,
    required_tools: Vec<String>,
    markdown_body: &str,
) -> Result<String, String> {
    let skill_hash = deterministic_hash(name);
    let now = chrono::Utc::now().to_rfc3339();

    // Build embedding text
    let embed_text = format!("{} {} {}", name, description, intents.join(" "));
    let skill_embedding = embedder.get_embedding(&embed_text).await;

    let mut skill_data = json!({
        "name": name,
        "description": description,
        "intents": intents,
        "suggested_tools": required_tools.iter().map(|t| json!({"name": t})).collect::<Vec<Value>>(),
        "source_path": "__learned__",
        "is_local": false,
        "indexed_at": now,
    });
    if let Some(emb) = &skill_embedding {
        skill_data["embedding"] = json!(emb);
    }

    // Upsert skill record
    let _ = db.query(&format!("UPSERT skill:⟨{}⟩ CONTENT $data", skill_hash))
        .bind(("data", skill_data))
        .await
        .map_err(|e| e.to_string())?
        .check()
        .map_err(|e| e.to_string())?;

    // Delete old chunks
    let _ = db.query("DELETE skill_chunk WHERE skill_id = $sid")
        .bind(("sid", format!("skill:⟨{}⟩", skill_hash)))
        .await
        .map_err(|e| e.to_string())?;

    // Chunk and embed new body
    let chunks = chunk_markdown(markdown_body, 2048, 128);
    for (idx, chunk_text) in chunks.iter().enumerate() {
        let chunk_hash = deterministic_hash(&format!("{}_{}", name, idx));
        let chunk_emb = embedder.get_embedding(chunk_text).await;

        let mut chunk_data = json!({
            "skill_id": format!("skill:⟨{}⟩", skill_hash),
            "text": chunk_text,
            "chunk_index": idx,
        });
        if let Some(emb) = &chunk_emb {
            chunk_data["embedding"] = json!(emb);
        }

        let _ = db.query(&format!("UPSERT skill_chunk:⟨{}⟩ CONTENT $data", chunk_hash))
            .bind(("data", chunk_data))
            .await
            .map_err(|e| e.to_string())?
            .check()
            .map_err(|e| e.to_string())?;
    }

    // Rebuild REQUIRES edges for this skill
    // First delete existing edges from this skill
    let _ = db.query(&format!("DELETE requires WHERE in = skill:⟨{}⟩", skill_hash))
        .await
        .map_err(|e| e.to_string())?;

    // Create new edges for each required tool
    for tool_name in &required_tools {
        // Try to find the tool by name
        let mut tool_res = db.query("SELECT id FROM tool WHERE name = $tool_name LIMIT 1")
            .bind(("tool_name", tool_name.clone()))
            .await
            .map_err(|e| e.to_string())?;

        let tool_rows: Vec<Value> = tool_res.take(0).unwrap_or_default();
        if let Some(tool_row) = tool_rows.first() {
            if let Some(tool_id) = tool_row.get("id") {
                let edge_input = format!("{}_{}", skill_hash, tool_name);
                let edge_hash = deterministic_hash(&edge_input);
                let _ = db.query(&format!(
                    "UPSERT requires:⟨{}⟩ CONTENT {{ in: skill:⟨{}⟩, out: $tool_id, source: 'explicit', confidence: 1.0 }}",
                    edge_hash, skill_hash
                ))
                .bind(("tool_id", tool_id.clone()))
                .await
                .map_err(|e| e.to_string())?;
            }
        }
    }

    Ok(format!("Learned skill '{}' with {} chunks and {} tool links.", name, chunks.len(), required_tools.len()))
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_frontmatter_valid() {
        let content = r#"---
name: "test-skill"
description: "A test skill for deployment"
intents:
  - "deploy application"
  - "manage servers"
suggested_tools:
  - name: "kubectl"
    type: "native_capability"
---
# Test Skill

This is the body of the skill.
"#;
        let (fm, body) = parse_frontmatter(content);
        assert_eq!(fm.name.unwrap(), "test-skill");
        assert_eq!(fm.description.unwrap(), "A test skill for deployment");
        assert_eq!(fm.intents.len(), 2);
        assert_eq!(fm.suggested_tools.len(), 1);
        assert!(body.starts_with("# Test Skill"));
    }

    #[test]
    fn test_parse_frontmatter_missing() {
        let content = "# Just a markdown file\n\nNo frontmatter here.";
        let (fm, body) = parse_frontmatter(content);
        assert!(fm.name.is_none());
        assert_eq!(body, content);
    }

    #[test]
    fn test_parse_frontmatter_malformed() {
        let content = "---\ninvalid: [yaml: broken\n---\n# Body";
        let (fm, body) = parse_frontmatter(content);
        assert!(fm.name.is_none());
        // Malformed YAML falls back to full content
        assert!(body.contains("invalid"));
    }

    #[test]
    fn test_chunk_markdown_small() {
        let text = "Hello world, this is a short text.";
        let chunks = chunk_markdown(text, 2048, 128);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], text);
    }

    #[test]
    fn test_chunk_markdown_large() {
        let text = "a".repeat(5000);
        let chunks = chunk_markdown(&text, 2048, 128);
        assert!(chunks.len() >= 3);
        // First chunk should be 2048 chars
        assert_eq!(chunks[0].len(), 2048);
        // Verify overlap: second chunk starts at 2048 - 128 = 1920
        assert_eq!(chunks[1].len(), 2048);
    }

    #[test]
    fn test_deterministic_hashing() {
        let h1 = deterministic_hash("test-skill");
        let h2 = deterministic_hash("test-skill");
        let h3 = deterministic_hash("other-skill");
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
    }
}
