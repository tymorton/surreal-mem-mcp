use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;

pub fn get_rules_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());

    let path = PathBuf::from(home).join(".surreal-mem-mcp").join("rules");

    // Ensure the directory exists
    let _ = fs::create_dir_all(&path);

    // Create default files if they don't exist
    let soul_path = path.join("SOUL.md");
    if !soul_path.exists() {
        let _ = fs::write(
            &soul_path,
            "# Soul\nYou are a helpful AI assistant empowered by Surreal-Mem-MCP. Use your memory tools to recall facts.",
        );
    }

    let memory_path = path.join("MEMORY.md");
    if !memory_path.exists() {
        let _ = fs::write(
            &memory_path,
            "# Learned Rules\nObserve user preferences and record them here using the update_behavioral_rules tool.",
        );
    }

    path
}

pub fn list_resources() -> Vec<Value> {
    vec![
        json!({
            "uri": "memory://rules/soul",
            "name": "Core Identity (SOUL.md)",
            "description": "The fundamental identity, behavior, and structural rules for the AI. Always read on boot."
        }),
        json!({
            "uri": "memory://rules/learned",
            "name": "Working Memory (MEMORY.md)",
            "description": "Dynamically learned user preferences and project-specific technical rules."
        }),
    ]
}

pub fn read_resource(uri: &str) -> Option<Value> {
    let rules_dir = get_rules_dir();

    let path = match uri {
        "memory://rules/soul" => rules_dir.join("SOUL.md"),
        "memory://rules/learned" => rules_dir.join("MEMORY.md"),
        _ => return None,
    };

    let content = fs::read_to_string(&path)
        .unwrap_or_else(|_| format!("Error: Unable to read resource at {:?}", path));

    Some(json!({
        "contents": [
            {
                "uri": uri,
                "mimeType": "text/markdown",
                "text": content
            }
        ]
    }))
}
