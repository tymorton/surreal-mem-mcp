use serde_json::{json, Value};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use surrealdb::engine::any::Any;
use surrealdb::Surreal;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::time::timeout;
use reqwest::Client;

// ── execute_capability ───────────────────────────────────────────────────

/// Execute a linked capability directly from the memory proxy layer.
pub async fn execute_capability(
    db: Arc<Surreal<Any>>,
    tool_id: &str,
    arguments: Value,
) -> Result<Value, String> {
    
    let raw_id = tool_id.replace("tool:", "").replace("`", "").replace("⟨", "").replace("⟩", "");
    let stored_tid = format!("tool:⟨{}⟩", raw_id);

    // Fetch the tool record
    let mut tool_res = db
        .query("SELECT * FROM tool WHERE id = type::record($stored_tid)")
        .bind(("stored_tid", stored_tid.clone()))
        .await
        .map_err(|e| e.to_string())?;

    let tool_rows: Vec<Value> = tool_res.take(0).unwrap_or_default();
    let tool = tool_rows
        .first()
        .ok_or_else(|| format!("Error: Tool not found: {}", tool_id))?;

    let tool_type = tool.get("type").and_then(|t| t.as_str()).unwrap_or("unknown");
    let empty_ctx = json!({});
    let execution_context = tool.get("execution_context").unwrap_or(&empty_ctx);
    let name = tool.get("name").and_then(|n| n.as_str()).unwrap_or("unknown");

    let start_time = std::time::Instant::now();

    let result = match tool_type {
        "local_script" => execute_local_script(name, execution_context, &arguments).await,
        "api_endpoint" => execute_api_endpoint(name, execution_context, &arguments).await,
        "mcp_server" => execute_mcp_server(name, execution_context, &arguments).await,
        "native_capability" => Ok(json!({
            "error": format!("This is a native capability. Please use your built-in tool '{}' directly.", name)
        })),
        _ => Err(format!("Unsupported tool type: {}", tool_type)),
    };

    let duration = start_time.elapsed().as_millis() as u64;

    let mut success = true;
    let mut error_msg: Option<String> = None;

    match &result {
        Ok(val) => {
            if let Some(err) = val.get("error").and_then(|e| e.as_str()) {
                success = false;
                error_msg = Some(err.to_string());
            } else if val.get("result").and_then(|r| r.get("isError")).and_then(|e| e.as_bool()) == Some(true) {
                success = false;
                
                // Try to extract the inner error message if available
                let inner_msg = val.get("result")
                    .and_then(|r| r.get("content"))
                    .and_then(|c| c.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|item| item.get("text"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("Tool returned an error flag");
                    
                error_msg = Some(inner_msg.to_string());
            } else if val.get("isError").and_then(|e| e.as_bool()) == Some(true) {
                success = false;
                error_msg = Some("Tool returned an error flag".to_string());
            }
        }
        Err(e) => {
            success = false;
            error_msg = Some(e.to_string());
        }
    }

    // Record Temporal Edge
    let edge_query = r#"
        UPSERT session:current CONTENT {};
        LET $target = type::record($stored_tid);
        RELATE session:current -> EXECUTED -> $target CONTENT {
            time: time::now(),
            success: $success,
            duration_ms: $duration,
            error_msg: $error_msg
        };
    "#;
    let _edge_res = db.query(edge_query)
        .bind(("stored_tid", stored_tid.clone()))
        .bind(("success", success))
        .bind(("duration", duration))
        .bind(("error_msg", error_msg.clone()))
        .await;

    let check_query = r#"
        LET $target = type::record($stored_tid);
        LET $recent = (SELECT success, time FROM EXECUTED WHERE out = $target ORDER BY time DESC LIMIT 3);
        IF array::len($recent) == 3 AND $recent[0].success == false AND $recent[1].success == false AND $recent[2].success == false THEN
            UPDATE $target SET status = 'degraded'
        ELSE IF $success THEN
            UPDATE $target SET status = 'active'
        END;
        RETURN $recent;
    "#;
    let _check_res = db.query(check_query)
        .bind(("stored_tid", stored_tid.clone()))
        .bind(("success", success))
        .await;

    result
}

// ── Local Script Execution ───────────────────────────────────────────────

async fn execute_local_script(name: &str, ctx: &Value, args: &Value) -> Result<Value, String> {
    let command_str = ctx.get("command").and_then(|c| c.as_str()).unwrap_or("echo");
    
    // Inject literal string arguments by templating `{arg_name}`
    let mut final_args: Vec<String> = Vec::new();
    if let Some(ctx_args) = ctx.get("args").and_then(|a| a.as_array()) {
        for arg_val in ctx_args {
            if let Some(arg_str) = arg_val.as_str() {
                let mut replaced = arg_str.to_string();
                if let Some(obj) = args.as_object() {
                    for (k, v) in obj {
                        let placeholder = format!("{{{}}}", k);
                        let val_str = if v.is_string() {
                            v.as_str().unwrap().to_string()
                        } else {
                            v.to_string()
                        };
                        replaced = replaced.replace(&placeholder, &val_str);
                    }
                }
                final_args.push(replaced);
            }
        }
    }

    let mut cmd = Command::new(command_str);
    cmd.args(final_args); // Safely pass explicitly to binary avoiding shell injection
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    // Inject Working Directory
    if let Some(cwd) = ctx.get("working_directory").and_then(|d| d.as_str()) {
        cmd.current_dir(cwd);
    }

    // Inject Args as Env Vars strictly
    if let Some(obj) = args.as_object() {
        for (k, v) in obj {
            let val_str = if v.is_string() {
                v.as_str().unwrap().to_string()
            } else {
                v.to_string()
            };
            cmd.env(k, val_str);
        }
    }

    // Spawn with 60 sec timeout
    let child_future = cmd.output();
    let result = match timeout(Duration::from_secs(60), child_future).await {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let status = output.status.code().unwrap_or(-1);
            
            if status == 0 {
                json!({"output": stdout, "stderr": stderr, "exit_code": status})
            } else {
                json!({"error": format!("Script failed with exit code {}", status), "stdout": stdout, "stderr": stderr})
            }
        }
        Ok(Err(e)) => json!({"error": format!("Failed to execute process: {}", e)}),
        Err(_) => json!({"error": "Script execution timed out after 60 seconds."}),
    };

    Ok(result)
}

// ── API Endpoint Execution ───────────────────────────────────────────────

async fn execute_api_endpoint(name: &str, ctx: &Value, args: &Value) -> Result<Value, String> {
    let url = ctx.get("url").and_then(|u| u.as_str()).unwrap_or("");
    let method = ctx.get("method").and_then(|m| m.as_str()).unwrap_or("POST").to_uppercase();

    // Template URL placeholders (e.g. https://api.example.com/{id})
    let mut final_url = url.to_string();
    if let Some(obj) = args.as_object() {
        for (k, v) in obj {
            let placeholder = format!("{{{}}}", k);
            let val_str = if v.is_string() {
                v.as_str().unwrap().to_string()
            } else {
                v.to_string()
            };
            final_url = final_url.replace(&placeholder, &val_str);
        }
    }

    let client = Client::new();
    let mut request = match method.as_str() {
        "GET" => client.get(&final_url),
        "POST" => client.post(&final_url),
        "PUT" => client.put(&final_url),
        "DELETE" => client.delete(&final_url),
        _ => return Err(format!("Unsupported HTTP method: {}", method)),
    };

    if let Some(headers) = ctx.get("headers").and_then(|h| h.as_object()) {
        for (k, v) in headers {
            if let Some(v_str) = v.as_str() {
                request = request.header(k, v_str);
            }
        }
    }

    // Inject Args as Body
    request = request.json(args);

    match request.send().await {
        Ok(res) => {
            let status = res.status().as_u16();
            let body = res.text().await.unwrap_or_default();
            Ok(json!({ "status": status, "response": body }))
        }
        Err(e) => Ok(json!({ "error": format!("API request failed: {}", e) }))
    }
}

// ── MCP Server STDIO Initialization/Execution ───────────────────────────────

async fn execute_mcp_server(name: &str, ctx: &Value, args: &Value) -> Result<Value, String> {
    let command_str = ctx.get("command").and_then(|c| c.as_str());
    if command_str.is_none() {
        return Ok(json!({"error": "MCP standard I/O command is missing from execution context"}));
    }
    let command_str = command_str.unwrap();

    let mut args_vec: Vec<String> = Vec::new();
    if let Some(ctx_args) = ctx.get("args").and_then(|a| a.as_array()) {
        for a in ctx_args {
            if let Some(s) = a.as_str() {
                args_vec.push(s.to_string());
            }
        }
    }

    let mut cmd = Command::new(command_str);
    cmd.args(args_vec);
    
    // Some MCP servers demand specific ENV setup inside execution context
    if let Some(env_map) = ctx.get("env").and_then(|e| e.as_object()) {
        for (k, v) in env_map {
            if let Some(v_str) = v.as_str() {
                cmd.env(k, v_str);
            }
        }
    }

    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return Ok(json!({"error": format!("Failed to spawn MCP process: {}", e)})),
    };

    let mut stdin = child.stdin.take().ok_or("Failed to open MCP stdin")?;
    let stdout = child.stdout.take().ok_or("Failed to open MCP stdout")?;
    
    // First we must send JSON-RPC initialization since this is a fresh connection
    let init_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "surreal-mem-proxy",
                "version": "0.1.0"
            }
        }
    });

    let mut msg_init = serde_json::to_string(&init_req).unwrap();
    msg_init.push('\n');
    let _ = stdin.write_all(msg_init.as_bytes()).await;
    let _ = stdin.flush().await;

    // Send the actual execution message
    let call_req = json!({
        "jsonrpc": "2.0",
        "id": 2, // Map to id=2 to await easily below
        "method": "tools/call",
        "params": {
            "name": name,
            "arguments": args
        }
    });
    let mut msg_call = serde_json::to_string(&call_req).unwrap();
    msg_call.push('\n');
    let _ = stdin.write_all(msg_call.as_bytes()).await;
    let _ = stdin.flush().await;

    // Read responses line-by-line using a BufReader
    let mut reader = BufReader::new(stdout).lines();

    let timeout_duration = Duration::from_secs(45);
    let target_result: Option<Value> = match timeout(timeout_duration, async {
        let mut target = None;
        while let Ok(Some(line)) = reader.next_line().await {
            if let Ok(parsed) = serde_json::from_str::<Value>(&line) {
                // Return gracefully if it matched our message ID 2
                if parsed.get("id").and_then(|id| id.as_u64()) == Some(2) {
                    target = Some(parsed);
                    break;
                }
            }
        }
        target
    }).await {
        Ok(res) => res,
        Err(_) => {
            let _ = child.kill().await;
            return Ok(json!({"error": "MCP Server stdio timeout after 45 seconds"}));
        }
    };

    // Attempt to terminate gracefully
    let _ = child.kill().await;

    if let Some(res) = target_result {
        // Return exactly what the MCP server returned (can contain `result` or `error` key)
        Ok(res)
    } else {
        Ok(json!({"error": "MCP server disconnected before returning result."}))
    }
}
