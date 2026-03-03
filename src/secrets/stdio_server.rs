// MCP stdio JSON-RPC server for the secrets vault.
// Launched by Claude CLI as an MCP subprocess; communicates via stdin/stdout.
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};

use super::mcp_server::SecretsVaultServer;

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    id: Option<serde_json::Value>,
    method: String,
    params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

impl JsonRpcResponse {
    fn ok(id: Option<serde_json::Value>, result: serde_json::Value) -> Self {
        Self { jsonrpc: "2.0", id, result: Some(result), error: None }
    }

    fn err(id: Option<serde_json::Value>, code: i32, msg: &str) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError { code, message: msg.to_string() }),
        }
    }
}

/// Run the MCP vault as a stdio JSON-RPC server (blocking the current async task).
/// Reads requests from stdin, writes responses to stdout — one JSON object per line.
pub async fn run_stdio_server(vault_root: &str, agent_name: &str) -> Result<()> {
    let server = SecretsVaultServer::new(vault_root, agent_name)?;

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let mut out = BufWriter::new(stdout);
    let mut lines = BufReader::new(stdin).lines();

    while let Some(line) = lines.next_line().await? {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let req: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(_) => continue,
        };

        // Notifications (no id) need no response
        let Some(id) = req.id.clone() else { continue };

        let resp = dispatch(&server, id, &req.method, req.params.as_ref());

        let json = serde_json::to_string(&resp)?;
        out.write_all(json.as_bytes()).await?;
        out.write_all(b"\n").await?;
        out.flush().await?;
    }

    Ok(())
}

fn dispatch(
    server: &SecretsVaultServer,
    id: serde_json::Value,
    method: &str,
    params: Option<&serde_json::Value>,
) -> JsonRpcResponse {
    match method {
        "initialize" => JsonRpcResponse::ok(
            Some(id),
            serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "claude-chat-vault", "version": "0.1.0"}
            }),
        ),
        "tools/list" => JsonRpcResponse::ok(
            Some(id),
            serde_json::json!({
                "tools": [{
                    "name": "get_secret",
                    "description": "Fetch a secret by name. Returns age-encrypted ciphertext (base64). Decrypt with your agent key.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string", "description": "Secret name"}
                        },
                        "required": ["name"]
                    }
                }]
            }),
        ),
        "tools/call" => {
            let p = params.and_then(|v| v.as_object());
            let tool = p.and_then(|p| p.get("name")).and_then(|v| v.as_str()).unwrap_or("");

            if tool == "get_secret" {
                let secret_name = p
                    .and_then(|p| p.get("arguments"))
                    .and_then(|a| a.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                match server.handle_get_secret(secret_name) {
                    Ok(enc) => JsonRpcResponse::ok(
                        Some(id),
                        serde_json::json!({
                            "content": [{"type": "text", "text": enc}],
                            "isError": false
                        }),
                    ),
                    Err(e) => JsonRpcResponse::ok(
                        Some(id),
                        serde_json::json!({
                            "content": [{"type": "text", "text": format!("error: {e}")}],
                            "isError": true
                        }),
                    ),
                }
            } else {
                JsonRpcResponse::err(Some(id), -32601, "unknown tool")
            }
        }
        _ => JsonRpcResponse::err(Some(id), -32601, &format!("method not found: {method}")),
    }
}
