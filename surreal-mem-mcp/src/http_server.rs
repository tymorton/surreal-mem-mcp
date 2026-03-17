use axum::{
    Json, Router,
    extract::{Query, State},
    response::{
        IntoResponse,
        sse::{Event, Sse},
    },
    routing::{get, post},
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
    pub session_id: String,
}

pub fn create_router(db: Arc<SurrealClient>) -> Router {
    let state = AppState {
        db,
        clients: Arc::new(RwLock::new(HashMap::new())),
    };

    Router::new()
        .route("/sse", get(sse_handler))
        .route("/message", post(message_handler))
        .with_state(state)
}

async fn sse_handler(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = mpsc::channel(100);
    let session_id = Uuid::new_v4().to_string();

    state
        .clients
        .write()
        .await
        .insert(session_id.clone(), tx.clone());

    let endpoint_url = format!("/message?session_id={}", session_id);
    let init_event = Event::default().event("endpoint").data(endpoint_url);

    let _ = tx.send(Ok(init_event)).await;

    Sse::new(ReceiverStream::new(rx))
}

async fn message_handler(
    State(state): State<AppState>,
    Query(query): Query<SessionQuery>,
    Json(payload): Json<JsonRpcRequest>,
) -> impl IntoResponse {
    let session_id = query.session_id.clone();

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
                        "version": "0.1.0"
                    }
                })),
                error: None,
            },
            "notifications/initialized" => {
                // Return early without sending an event back to the client natively
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
                let result =
                    tools::call_tool(payload.params.unwrap_or(json!({})), state.db.clone()).await;
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

    axum::http::StatusCode::ACCEPTED
}
