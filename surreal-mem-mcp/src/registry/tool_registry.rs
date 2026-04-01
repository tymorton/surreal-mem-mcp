use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;
use serde::Deserialize;
use serde_json::{Value, json};
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use crate::embeddings::local::LocalEmbedder;

// ── Config Types ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Clone)]
pub struct ToolRegistryConfig {
    #[serde(default)]
    pub mcp_servers: Vec<McpServerConfig>,
    #[serde(default)]
    pub tools: Vec<StaticToolConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct McpServerConfig {
    pub name: String,
    pub transport: String, // "stdio" or "sse"
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub url: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct StaticToolConfig {
    pub name: String,
    #[serde(rename = "type")]
    pub tool_type: String,
    pub description: String,
    pub execution_context: Value,
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn deterministic_hash(data: &str) -> String {
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    format!("{}", hasher.finish())
}

/// Expand environment variables in a JSON value.
/// Strings starting with `$` or wrapped in `${}` are resolved against host env.
/// Returns None if a required variable is missing (caller should skip the tool).
fn expand_env_vars(value: &Value) -> Result<Value, String> {
    match value {
        Value::String(s) => {
            let expanded = expand_env_string(s)?;
            Ok(Value::String(expanded))
        }
        Value::Object(map) => {
            let mut new_map = serde_json::Map::new();
            for (k, v) in map {
                let expanded = expand_env_vars(v)?;
                new_map.insert(k.clone(), expanded);
            }
            Ok(Value::Object(new_map))
        }
        Value::Array(arr) => {
            let mut new_arr = Vec::new();
            for v in arr {
                new_arr.push(expand_env_vars(v)?);
            }
            Ok(Value::Array(new_arr))
        }
        other => Ok(other.clone()),
    }
}

/// Expand `$VAR` or `${VAR}` patterns in a string.
fn expand_env_string(s: &str) -> Result<String, String> {
    let mut result = s.to_string();

    // Handle ${VAR} pattern
    while let Some(start) = result.find("${") {
        if let Some(end) = result[start..].find('}') {
            let var_name = &result[start + 2..start + end];
            let var_value = std::env::var(var_name)
                .map_err(|_| format!("Missing environment variable: {}", var_name))?;
            result = format!("{}{}{}", &result[..start], var_value, &result[start + end + 1..]);
        } else {
            break;
        }
    }

    // Handle $VAR pattern (only if the string is exactly "$VAR")
    if result.starts_with('$') && !result.starts_with("${") && !result.contains(' ') {
        let var_name = &result[1..];
        if !var_name.is_empty() {
            let var_value = std::env::var(var_name)
                .map_err(|_| format!("Missing environment variable: {}", var_name))?;
            result = var_value;
        }
    }

    Ok(result)
}

// ── Config File Location ────────────────────────────────────────────────

/// Collect all config file paths to process (global + local).
/// Returns Vec<(PathBuf, is_local)>.
fn collect_config_paths() -> Vec<(PathBuf, bool)> {
    let mut configs = Vec::new();

    // Env var override takes highest priority
    if let Ok(custom) = std::env::var("TOOL_REGISTRY_CONFIG") {
        let p = PathBuf::from(custom);
        if p.exists() {
            configs.push((p, false));
            return configs;
        }
    }

    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());

    // Global: check inbox first, then root config dir
    let inbox_path = PathBuf::from(&home)
        .join(".surreal-mem-mcp")
        .join("inbox")
        .join("tool_registry.json");
    if inbox_path.exists() {
        configs.push((inbox_path, false));
    } else {
        let root_path = PathBuf::from(&home)
            .join(".surreal-mem-mcp")
            .join("tool_registry.json");
        if root_path.exists() {
            configs.push((root_path, false));
        }
    }

    // Local: check project directories
    let cwd = std::env::current_dir().unwrap_or_default();
    let local_paths = [
        cwd.join("tool_registry.json"),
        cwd.join(".agents").join("tool_registry.json"),
        cwd.join(".mcp").join("tool_registry.json"),
    ];
    for p in &local_paths {
        if p.exists() {
            configs.push((p.clone(), true));
        }
    }

    configs
}

fn get_global_archive_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".surreal-mem-mcp").join("archive")
}

// ── MCP Server Auto-Discovery ───────────────────────────────────────────

/// Discover tools from a stdio MCP server by spawning it and sending JSON-RPC.
async fn discover_stdio_tools(server: &McpServerConfig) -> Result<Vec<Value>, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::process::Command;

    let command = server.command.as_deref()
        .ok_or_else(|| format!("stdio server '{}' missing 'command'", server.name))?;

    let args = server.args.clone().unwrap_or_default();

    let mut child = Command::new(command)
        .args(&args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to spawn '{}': {}", command, e))?;

    let mut stdin = child.stdin.take().ok_or("No stdin")?;
    let mut stdout = child.stdout.take().ok_or("No stdout")?;

    // Send initialize
    let init_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "surreal-mem-mcp-registry", "version": "0.5.0" }
        }
    });
    let init_msg = format!("{}\n", serde_json::to_string(&init_req).unwrap());
    stdin.write_all(init_msg.as_bytes()).await.map_err(|e| e.to_string())?;

    // Read initialize response (with timeout)
    let mut buf = vec![0u8; 65536];
    let read_result = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        stdout.read(&mut buf),
    ).await;

    match read_result {
        Ok(Ok(n)) if n > 0 => {
            // Parse to verify we got a valid response
            let _ = serde_json::from_slice::<Value>(&buf[..n]);
        }
        _ => {
            let _ = child.kill().await;
            return Err(format!("Timeout reading initialize response from '{}'", server.name));
        }
    }

    // Send initialized notification
    let notif = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
    let notif_msg = format!("{}\n", serde_json::to_string(&notif).unwrap());
    stdin.write_all(notif_msg.as_bytes()).await.map_err(|e| e.to_string())?;

    // Send tools/list
    let list_req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    });
    let list_msg = format!("{}\n", serde_json::to_string(&list_req).unwrap());
    stdin.write_all(list_msg.as_bytes()).await.map_err(|e| e.to_string())?;

    // Read tools/list response
    let mut buf2 = vec![0u8; 262144]; // 256KB for large tool lists
    let read_result = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        stdout.read(&mut buf2),
    ).await;

    let _ = child.kill().await;

    match read_result {
        Ok(Ok(n)) if n > 0 => {
            let response: Value = serde_json::from_slice(&buf2[..n])
                .map_err(|e| format!("JSON parse error from '{}': {}", server.name, e))?;

            if let Some(tools) = response.get("result").and_then(|r| r.get("tools")).and_then(|t| t.as_array()) {
                Ok(tools.clone())
            } else {
                Ok(vec![])
            }
        }
        _ => Err(format!("Timeout reading tools/list from '{}'", server.name)),
    }
}

/// Discover tools from an SSE MCP server via HTTP POST.
async fn discover_sse_tools(server: &McpServerConfig) -> Result<Vec<Value>, String> {
    let url = server.url.as_deref()
        .ok_or_else(|| format!("SSE server '{}' missing 'url'", server.name))?;

    let client = reqwest::Client::new();

    // Send initialize
    let init_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": { "name": "surreal-mem-mcp-registry", "version": "0.5.0" }
        }
    });

    let init_resp = client.post(url)
        .json(&init_req)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("HTTP error connecting to '{}': {}", server.name, e))?;

    let session_id = init_resp.headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // Send tools/list
    let list_req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    });

    let mut req_builder = client.post(url).json(&list_req);
    if let Some(sid) = &session_id {
        req_builder = req_builder.header("mcp-session-id", sid);
    }

    let list_resp = req_builder
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("HTTP error listing tools from '{}': {}", server.name, e))?;

    let response: Value = list_resp.json().await
        .map_err(|e| format!("JSON parse error from '{}': {}", server.name, e))?;

    if let Some(tools) = response.get("result").and_then(|r| r.get("tools")).and_then(|t| t.as_array()) {
        Ok(tools.clone())
    } else {
        Ok(vec![])
    }
}

// ── Main Sync Function ──────────────────────────────────────────────────

pub async fn sync_tool_registry(
    db: Arc<Surreal<Db>>,
    embedder: Arc<LocalEmbedder>,
) -> Result<String, String> {
    let config_paths = collect_config_paths();
    if config_paths.is_empty() {
        return Ok("No tool_registry.json found. Skipping tool sync.".to_string());
    }

    let mut synced_tool_ids: HashSet<String> = HashSet::new();
    let mut tool_count = 0;

    for (config_path, is_local) in &config_paths {
        let config_content = match std::fs::read_to_string(config_path) {
            Ok(c) => c,
            Err(e) => {
                println!("[tool_registry] Skipping {}: {}", config_path.display(), e);
                continue;
            }
        };

        let config: ToolRegistryConfig = match serde_json::from_str(&config_content) {
            Ok(c) => c,
            Err(e) => {
                println!("[tool_registry] Parse error in {}: {}", config_path.display(), e);
                continue;
            }
        };

        println!("[tool_registry] Processing {} (local={})", config_path.display(), is_local);

        // 1. Ingest static tools
        for tool_cfg in &config.tools {
            let exec_ctx = match expand_env_vars(&tool_cfg.execution_context) {
                Ok(expanded) => expanded,
                Err(e) => {
                    println!("[tool_registry] WARNING: Skipping tool '{}': {}", tool_cfg.name, e);
                    continue;
                }
            };

            let tool_hash = deterministic_hash(&format!("{}_{}", tool_cfg.tool_type, tool_cfg.name));
            synced_tool_ids.insert(format!("tool:⟨{}⟩", tool_hash));

            let embed_text = format!("{} {}", tool_cfg.name, tool_cfg.description);
            let tool_embedding = embedder.get_embedding(&embed_text).await;

            let now = chrono::Utc::now().to_rfc3339();
            let mut tool_data = json!({
                "name": tool_cfg.name,
                "description": tool_cfg.description,
                "type": tool_cfg.tool_type,
                "execution_context": exec_ctx,
                "source": if *is_local { "config:local" } else { "config" },
                "indexed_at": now,
            });
            if let Some(emb) = &tool_embedding {
                tool_data["embedding"] = json!(emb);
            }

            let _ = db.query(&format!("UPSERT tool:⟨{}⟩ CONTENT $data", tool_hash))
                .bind(("data", tool_data))
                .await
                .map_err(|e| e.to_string())?
                .check()
                .map_err(|e| e.to_string())?;

            tool_count += 1;
            println!("[tool_registry] Synced static tool: {} ({})", tool_cfg.name, tool_cfg.tool_type);
        }

        // 2. Auto-discover MCP server tools
        for server in &config.mcp_servers {
            println!("[tool_registry] Discovering tools from MCP server: {} ({})", server.name, server.transport);

            let tools_result = match server.transport.as_str() {
                "stdio" => discover_stdio_tools(server).await,
                "sse" => discover_sse_tools(server).await,
                other => {
                    println!("[tool_registry] WARNING: Unknown transport '{}' for server '{}'", other, server.name);
                    continue;
                }
            };

            match tools_result {
                Ok(tools) => {
                    for tool in &tools {
                        let tool_name = tool.get("name").and_then(|n| n.as_str()).unwrap_or("unknown");
                        let tool_desc = tool.get("description").and_then(|d| d.as_str()).unwrap_or("");
                        let input_schema = tool.get("inputSchema").cloned().unwrap_or(json!({}));

                        let tool_hash = deterministic_hash(&format!("mcp_server_{}_{}", server.name, tool_name));
                        synced_tool_ids.insert(format!("tool:⟨{}⟩", tool_hash));

                        let embed_text = format!("{} {}", tool_name, tool_desc);
                        let tool_embedding = embedder.get_embedding(&embed_text).await;

                        let now = chrono::Utc::now().to_rfc3339();
                        let exec_ctx = json!({
                            "server_name": server.name,
                            "transport": server.transport,
                            "command": server.command,
                            "args": server.args,
                            "url": server.url,
                            "input_schema": input_schema,
                        });

                        let mut tool_data = json!({
                            "name": tool_name,
                            "description": tool_desc,
                            "type": "mcp_server",
                            "execution_context": exec_ctx,
                            "source": format!("mcp:{}", server.name),
                            "indexed_at": now,
                        });
                        if let Some(emb) = &tool_embedding {
                            tool_data["embedding"] = json!(emb);
                        }

                        let _ = db.query(&format!("UPSERT tool:⟨{}⟩ CONTENT $data", tool_hash))
                            .bind(("data", tool_data))
                            .await
                            .map_err(|e| e.to_string())?
                            .check()
                            .map_err(|e| e.to_string())?;

                        tool_count += 1;
                    }
                    println!("[tool_registry] Discovered {} tools from '{}'", tools.len(), server.name);
                }
                Err(e) => {
                    println!("[tool_registry] WARNING: Failed to discover tools from '{}': {}", server.name, e);
                }
            }
        }

        // 3. Archive: move config file after successful parse
        let archive_dir = if *is_local {
            // Local config → local archive (sibling directory)
            config_path.parent()
                .map(|p| p.join("archive"))
                .unwrap_or_else(|| {
                    let cwd = std::env::current_dir().unwrap_or_default();
                    cwd.join(".agents").join("archive")
                })
        } else {
            get_global_archive_dir()
        };

        let _ = std::fs::create_dir_all(&archive_dir);

        // Ensure ignore files
        let mut ignore_files: Vec<&str> = vec![".cursorignore", ".aiderignore"];
        if *is_local {
            ignore_files.push(".gitignore");
        }
        for filename in &ignore_files {
            let ignore_path = archive_dir.join(filename);
            if !ignore_path.exists() {
                let _ = std::fs::write(&ignore_path, "*\n");
            }
        }

        let dest = archive_dir.join("tool_registry.json");
        // Timestamp if already exists
        let final_dest = if dest.exists() {
            let ts = chrono::Utc::now().format("%Y%m%d%H%M%S");
            archive_dir.join(format!("tool_registry__{}.json", ts))
        } else {
            dest
        };

        if let Err(e) = std::fs::rename(config_path, &final_dest) {
            if std::fs::copy(config_path, &final_dest).is_ok() {
                let _ = std::fs::remove_file(config_path);
                println!("[tool_registry] Archived: {} → {}", config_path.display(), final_dest.display());
            } else {
                println!("[tool_registry] Archive warning: {}", e);
            }
        } else {
            println!("[tool_registry] Archived: {} → {}", config_path.display(), final_dest.display());
        }
    }

    // 4. Config pruning: delete tools that are config-sourced but not in current sync
    // let synced_ids_vec: Vec<String> = synced_tool_ids.into_iter().collect();
    // let _ = db.query(
    //    "DELETE tool WHERE (source = 'config' OR source = 'config:local' OR source CONTAINS 'mcp:') AND <string>id NOT IN $synced_ids"
    // )
    // .bind(("synced_ids", synced_ids_vec))
    // .await;

    Ok(format!("Synced {} tools from registry.", tool_count))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_env_vars_simple() {
        unsafe { std::env::set_var("TEST_TOOL_VAR", "hello123"); }
        let input = json!({
            "url": "https://api.example.com",
            "token": "$TEST_TOOL_VAR",
            "nested": { "key": "${TEST_TOOL_VAR}" }
        });
        let expanded = expand_env_vars(&input).unwrap();
        assert_eq!(expanded["token"], "hello123");
        assert_eq!(expanded["nested"]["key"], "hello123");
        assert_eq!(expanded["url"], "https://api.example.com");
        unsafe { std::env::remove_var("TEST_TOOL_VAR"); }
    }

    #[test]
    fn test_expand_env_vars_missing() {
        let input = json!({ "token": "$NONEXISTENT_VAR_XYZ" });
        let result = expand_env_vars(&input);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Missing environment variable"));
    }

    #[test]
    fn test_config_paths_default() {
        unsafe { std::env::remove_var("TOOL_REGISTRY_CONFIG"); }
        // Just verify the function runs without panicking
        let paths = collect_config_paths();
        // Paths may or may not exist depending on the test environment
        let _ = paths;
    }

    #[test]
    fn test_config_path_override() {
        unsafe { std::env::set_var("TOOL_REGISTRY_CONFIG", "/tmp/custom_registry.json"); }
        let paths = collect_config_paths();
        // This file doesn't exist, so it won't be returned
        // But the function should not panic
        let _ = paths;
        unsafe { std::env::remove_var("TOOL_REGISTRY_CONFIG"); }
    }
}
