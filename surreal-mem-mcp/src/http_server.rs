use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{
        IntoResponse, Response,
        sse::{Event, Sse},
    },
    routing::{delete, get, post},
};
use futures::stream::Stream;
use serde::Deserialize;
use serde_json::json;
use std::{collections::HashMap, convert::Infallible, sync::Arc};
use tokio::sync::{RwLock, mpsc};
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

use crate::mcp_types::{JsonRpcRequest, JsonRpcResponse};
use crate::surreal_client::SurrealClient;
use crate::tools;

type ClientMap = Arc<RwLock<HashMap<String, mpsc::Sender<Result<Event, Infallible>>>>>;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<SurrealClient>,
    pub clients: ClientMap,
}

#[derive(Deserialize)]
pub struct SessionQuery {
    pub session_id: Option<String>,
}

pub fn create_router(db: Arc<SurrealClient>) -> Router {
    let state = AppState {
        db,
        clients: Arc::new(RwLock::new(HashMap::new())),
    };

    Router::new()
        .route("/", get(info_handler))
        // Streamable HTTP transport (MCP 2025-03-26):
        //   POST /sse  → synchronous JSON-RPC handler (initialize, tools/call, etc.)
        //   GET  /sse  → SSE stream for server-initiated pushes
        .route("/sse", post(streamable_post_handler))
        .route("/sse", get(sse_handler))
        // Legacy SSE transport: client POSTs to /message?session_id=<uuid>
        .route("/message", post(message_handler))
        .route("/session/:session_id", delete(session_terminate_handler))
        .with_state(state)
}

/// GET / — Transport info endpoint.
async fn info_handler() -> impl IntoResponse {
    Json(json!({
        "server": "surreal-mem-mcp",
        "transport": ["streamable-http", "sse"],
        "endpoint": "/sse",
        "note": "Supports both MCP 2025-03-26 Streamable HTTP (POST /sse) and legacy SSE transport."
    }))
}

/// DELETE /session/:session_id — Orchestrator-side session lifecycle hook.
async fn session_terminate_handler(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    match state.db.end_session(&session_id).await {
        Ok(_) => (
            StatusCode::OK,
            Json(json!({ "deleted": true, "session_id": session_id })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

/// POST /sse — MCP 2025-03-26 Streamable HTTP transport.
///
/// Handles JSON-RPC requests synchronously, returning the response directly
/// in the HTTP response body. Generates (or reuses) an `Mcp-Session-Id` that
/// the client must echo on all subsequent requests.
async fn streamable_post_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<JsonRpcRequest>,
) -> Response {
    // Honour existing session or create a new one.
    let session_id = headers
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    // Extract optional Openclaw middleware headers (agent / session propagation).
    let header_agent_id = headers
        .get("x-agent-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let header_session_id = headers
        .get("x-session-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let response: JsonRpcResponse = match payload.method.as_str() {
        "initialize" => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: payload.id.clone(),
            result: Some(json!({
                "protocolVersion": "2025-03-26",
                "capabilities": {
                    "tools": {},
                    "resources": {}
                },
                "serverInfo": {
                    "name": "surreal-mem-mcp",
                    "version": "0.2.0"
                }
            })),
            error: None,
        },

        "notifications/initialized" => {
            // Client acknowledgement — no response body needed, just 200.
            return StatusCode::OK.into_response();
        }

        "tools/list" => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: payload.id.clone(),
            result: Some(json!({
                "tools": tools::list_tools()
            })),
            error: None,
        },

        "tools/call" => {
            let mut params = payload.params.unwrap_or(json!({}));
            if let Some(args) = params.get_mut("arguments") {
                if args.get("agent_id").is_none() || args["agent_id"].is_null() {
                    if let Some(ref aid) = header_agent_id {
                        args["agent_id"] = json!(aid);
                    }
                }
                if args.get("session_id").is_none() || args["session_id"].is_null() {
                    if let Some(ref sid) = header_session_id {
                        args["session_id"] = json!(sid);
                    }
                }
                if args.get("author_agent_id").is_none() || args["author_agent_id"].is_null() {
                    if let Some(ref aid) = header_agent_id {
                        args["author_agent_id"] = json!(aid);
                    }
                }
            }
            let result = tools::call_tool(params, state.db.clone()).await;
            JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: payload.id.clone(),
                result: Some(result),
                error: None,
            }
        }

        "resources/list" => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: payload.id.clone(),
            result: Some(json!({
                "resources": crate::resources::list_resources()
            })),
            error: None,
        },

        "resources/read" => {
            let uri = payload
                .params
                .as_ref()
                .and_then(|p| p.get("uri"))
                .and_then(|u| u.as_str())
                .unwrap_or("");

            match crate::resources::read_resource(uri) {
                Some(res) => JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: payload.id.clone(),
                    result: Some(res),
                    error: None,
                },
                None => JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: payload.id.clone(),
                    result: None,
                    error: Some(json!({
                        "code": -32602,
                        "message": format!("Resource not found: {}", uri)
                    })),
                },
            }
        }

        "ping" => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: payload.id.clone(),
            result: Some(json!({})),
            error: None,
        },

        _ => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: payload.id.clone(),
            result: None,
            error: Some(json!({
                "code": -32601,
                "message": format!("Method not found: {}", payload.method)
            })),
        },
    };

    // Build response with Mcp-Session-Id so the client tracks the session.
    let body = serde_json::to_string(&response).unwrap_or_default();
    let mut resp = (StatusCode::OK, body).into_response();
    resp.headers_mut().insert(
        "content-type",
        HeaderValue::from_static("application/json"),
    );
    resp.headers_mut().insert(
        "mcp-session-id",
        HeaderValue::from_str(&session_id).unwrap_or_else(|_| HeaderValue::from_static("unknown")),
    );
    resp
}

/// GET /sse — Opens an SSE stream.
///
/// Supports both legacy (no session header, sends `endpoint` event) and
/// Streamable HTTP (with `Mcp-Session-Id` header, for server-push messages).
async fn sse_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = mpsc::channel(100);

    // If the client provides an existing session ID, attach to it.
    // Otherwise mint a new one (legacy SSE flow).
    let session_id = headers
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    state
        .clients
        .write()
        .await
        .insert(session_id.clone(), tx.clone());

    // Legacy SSE clients need the `endpoint` bootstrap event.
    let is_legacy = headers.get("mcp-session-id").is_none();
    if is_legacy {
        let endpoint_url = format!("/message?session_id={}", session_id);
        let init_event = Event::default().event("endpoint").data(endpoint_url);
        let _ = tx.send(Ok(init_event)).await;
    }

    Sse::new(ReceiverStream::new(rx))
}

/// POST /message?session_id=<uuid> — Legacy SSE transport message endpoint.
///
/// Kept for backward compatibility. Dispatches JSON-RPC to the session's SSE
/// channel and immediately returns 202 Accepted.
async fn message_handler(
    State(state): State<AppState>,
    Query(query): Query<SessionQuery>,
    headers: HeaderMap,
    Json(payload): Json<JsonRpcRequest>,
) -> impl IntoResponse {
    let header_agent_id = headers
        .get("x-agent-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let header_session_id = headers
        .get("x-session-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let session_id = query.session_id.clone().unwrap_or_default();

    tokio::spawn(async move {
        let response = match payload.method.as_str() {
            "initialize" => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: payload.id.clone(),
                result: Some(json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {
                        "tools": {},
                        "resources": {}
                    },
                    "serverInfo": {
                        "name": "surreal-mem-mcp",
                        "version": "0.2.0"
                    }
                })),
                error: None,
            },
            "notifications/initialized" => {
                return;
            }
            "tools/list" => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: payload.id.clone(),
                result: Some(json!({
                    "tools": tools::list_tools()
                })),
                error: None,
            },
            "tools/call" => {
                let mut params = payload.params.unwrap_or(json!({}));
                if let Some(args) = params.get_mut("arguments") {
                    if args.get("agent_id").is_none() || args["agent_id"].is_null() {
                        if let Some(ref aid) = header_agent_id {
                            args["agent_id"] = json!(aid);
                        }
                    }
                    if args.get("session_id").is_none() || args["session_id"].is_null() {
                        if let Some(ref sid) = header_session_id {
                            args["session_id"] = json!(sid);
                        }
                    }
                    if args.get("author_agent_id").is_none() || args["author_agent_id"].is_null() {
                        if let Some(ref aid) = header_agent_id {
                            args["author_agent_id"] = json!(aid);
                        }
                    }
                }
                let result = tools::call_tool(params, state.db.clone()).await;
                JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: payload.id.clone(),
                    result: Some(result),
                    error: None,
                }
            }
            "resources/list" => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: payload.id.clone(),
                result: Some(json!({
                    "resources": crate::resources::list_resources()
                })),
                error: None,
            },
            "resources/read" => {
                let uri = payload
                    .params
                    .as_ref()
                    .and_then(|p| p.get("uri"))
                    .and_then(|u| u.as_str())
                    .unwrap_or("");
                match crate::resources::read_resource(uri) {
                    Some(res) => JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id: payload.id.clone(),
                        result: Some(res),
                        error: None,
                    },
                    None => JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id: payload.id.clone(),
                        result: None,
                        error: Some(json!({
                            "code": -32602,
                            "message": format!("Resource not found: {}", uri)
                        })),
                    },
                }
            }
            "ping" => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: payload.id.clone(),
                result: Some(json!({})),
                error: None,
            },
            _ => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: payload.id.clone(),
                result: None,
                error: Some(json!({
                    "code": -32601,
                    "message": format!("Method not found: {}", payload.method)
                })),
            },
        };

        let clients = state.clients.read().await;
        if let Some(tx) = clients.get(&session_id) {
            let event = Event::default()
                .event("message")
                .data(serde_json::to_string(&response).unwrap());
            let _ = tx.send(Ok(event)).await;
        }
    });

    StatusCode::ACCEPTED
}
