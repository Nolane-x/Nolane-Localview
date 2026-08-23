#![forbid(unsafe_code)]

use std::io::{self, BufRead, Write};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
struct RpcRequest {
    #[serde(rename = "jsonrpc")]
    _jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct RpcResponse {
    jsonrpc: &'static str,
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<Value>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: RpcRequest = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(error) => {
                writeln!(
                    stdout,
                    "{}",
                    serde_json::to_string(&RpcResponse {
                        jsonrpc: "2.0",
                        id: None,
                        result: None,
                        error: Some(json!({"code": -32700, "message": error.to_string()})),
                    })?
                )?;
                stdout.flush()?;
                continue;
            }
        };
        let response = handle(request).await;
        writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
        stdout.flush()?;
    }
    Ok(())
}

async fn handle(request: RpcRequest) -> RpcResponse {
    let id = request.id.clone();
    let result = match request.method.as_str() {
        "initialize" => Ok(json!({
            "protocolVersion": "2025-06-18",
            "serverInfo": {"name": "localview", "version": env!("CARGO_PKG_VERSION")},
            "capabilities": {"tools": {}}
        })),
        "tools/list" => Ok(json!({"tools": tool_definitions()})),
        "tools/call" => call_tool(&request.params).await,
        _ => Err(anyhow::anyhow!("method not found: {}", request.method)),
    };
    match result {
        Ok(value) => RpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(value),
            error: None,
        },
        Err(error) => RpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(json!({"code": -32000, "message": error.to_string()})),
        },
    }
}

fn tool_definitions() -> Vec<Value> {
    vec![
        json!({"name":"session.list","description":"List detected LocalView sessions","inputSchema":{"type":"object","properties":{}}}),
        json!({"name":"session.inspect","description":"Inspect one LocalView session","inputSchema":{"type":"object","properties":{"id":{"type":"string"}},"required":["id"]}}),
        json!({"name":"session.analysis","description":"Analyze retained live console, network and performance evidence for one session","inputSchema":{"type":"object","properties":{"session":{"type":"string"}},"required":["session"]}}),
        json!({"name":"session.diagnose","description":"Return evidence-first findings, uncertainty and recommended next checks for one session","inputSchema":{"type":"object","properties":{"session":{"type":"string"}},"required":["session"]}}),
        json!({"name":"evidence.recent","description":"Read recent content-addressed evidence objects for one session","inputSchema":{"type":"object","properties":{"session":{"type":"string"}},"required":["session"]}}),
        json!({"name":"evidence.get","description":"Read one evidence object by evidence id","inputSchema":{"type":"object","properties":{"id":{"type":"string"}},"required":["id"]}}),
        json!({"name":"runtime.pause","description":"Pause localhost discovery","inputSchema":{"type":"object","properties":{}}}),
        json!({"name":"runtime.resume","description":"Resume localhost discovery","inputSchema":{"type":"object","properties":{}}}),
        json!({"name":"events.recent","description":"Return recent daemon runtime events","inputSchema":{"type":"object","properties":{}}}),
        json!({"name":"observer.recent","description":"Read recent in-page observer events for one session","inputSchema":{"type":"object","properties":{"session":{"type":"string"}},"required":["session"]}}),
        json!({"name":"action.click","description":"Queue a click against a stable LocalView element reference","inputSchema":{"type":"object","properties":{"session":{"type":"string"},"reference":{"type":"string"}},"required":["session","reference"]}}),
        json!({"name":"action.type","description":"Queue text input against a stable element reference","inputSchema":{"type":"object","properties":{"session":{"type":"string"},"reference":{"type":"string"},"text":{"type":"string"},"clear_first":{"type":"boolean","default":false}},"required":["session","reference","text"]}}),
        json!({"name":"action.key","description":"Queue a keyboard event","inputSchema":{"type":"object","properties":{"session":{"type":"string"},"reference":{"type":"string"},"key":{"type":"string"},"modifiers":{"type":"array","items":{"type":"string"}}},"required":["session","key"]}}),
        json!({"name":"action.scroll","description":"Queue a deterministic scroll offset","inputSchema":{"type":"object","properties":{"session":{"type":"string"},"x":{"type":"number"},"y":{"type":"number"}},"required":["session","x","y"]}}),
        json!({"name":"action.focus","description":"Queue focus for a stable element reference","inputSchema":{"type":"object","properties":{"session":{"type":"string"},"reference":{"type":"string"}},"required":["session","reference"]}}),
        json!({"name":"action.snapshot","description":"Ask the page bridge for a semantic snapshot","inputSchema":{"type":"object","properties":{"session":{"type":"string"}},"required":["session"]}}),
        json!({"name":"action.results","description":"Read recent page action results","inputSchema":{"type":"object","properties":{"session":{"type":"string"}},"required":["session"]}}),
    ]
}

async fn call_tool(params: &Value) -> Result<Value> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .context("missing tool name")?;
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let base = std::env::var("LOCALVIEW_CONTROL")
        .unwrap_or_else(|_| "http://127.0.0.1:45454".into());
    let token = read_token().await?;
    let client = reqwest::Client::new();

    let response = match name {
        "session.list" => client
            .get(format!("{base}/v1/sessions"))
            .bearer_auth(&token)
            .send()
            .await?,
        "session.inspect" => {
            let id = string_arg(&args, "id")?;
            client
                .get(format!("{base}/v1/sessions/{id}"))
                .bearer_auth(&token)
                .send()
                .await?
        }
        "session.analysis" => {
            let session = string_arg(&args, "session")?;
            client
                .get(format!("{base}/v1/sessions/{session}/analysis"))
                .bearer_auth(&token)
                .send()
                .await?
        }
        "session.diagnose" => {
            let session = string_arg(&args, "session")?;
            client
                .get(format!("{base}/v1/sessions/{session}/diagnose"))
                .bearer_auth(&token)
                .send()
                .await?
        }
        "evidence.recent" => {
            let session = string_arg(&args, "session")?;
            client
                .get(format!("{base}/v1/sessions/{session}/evidence/recent"))
                .bearer_auth(&token)
                .send()
                .await?
        }
        "evidence.get" => {
            let id = string_arg(&args, "id")?;
            client
                .get(format!("{base}/v1/evidence/{id}"))
                .bearer_auth(&token)
                .send()
                .await?
        }
        "runtime.pause" => client
            .post(format!("{base}/v1/runtime/pause"))
            .bearer_auth(&token)
            .send()
            .await?,
        "runtime.resume" => client
            .post(format!("{base}/v1/runtime/resume"))
            .bearer_auth(&token)
            .send()
            .await?,
        "events.recent" => client
            .get(format!("{base}/v1/events/recent"))
            .bearer_auth(&token)
            .send()
            .await?,
        "observer.recent" => {
            let session = string_arg(&args, "session")?;
            client
                .get(format!("{base}/v1/sessions/{session}/observer/recent"))
                .bearer_auth(&token)
                .send()
                .await?
        }
        "action.click" => {
            let session = string_arg(&args, "session")?;
            let reference = string_arg(&args, "reference")?;
            post_action(
                &client,
                &base,
                &token,
                session,
                Some(reference),
                json!({"type":"click"}),
            )
            .await?
        }
        "action.type" => {
            let session = string_arg(&args, "session")?;
            let reference = string_arg(&args, "reference")?;
            let text = string_arg(&args, "text")?;
            let clear_first = args
                .get("clear_first")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            post_action(
                &client,
                &base,
                &token,
                session,
                Some(reference),
                json!({"type":"type_text","text":text,"clear_first":clear_first}),
            )
            .await?
        }
        "action.key" => {
            let session = string_arg(&args, "session")?;
            let key = string_arg(&args, "key")?;
            let reference = args.get("reference").and_then(Value::as_str);
            let modifiers = args
                .get("modifiers")
                .cloned()
                .unwrap_or_else(|| json!([]));
            post_action(
                &client,
                &base,
                &token,
                session,
                reference,
                json!({"type":"key","key":key,"modifiers":modifiers}),
            )
            .await?
        }
        "action.scroll" => {
            let session = string_arg(&args, "session")?;
            let x = number_arg(&args, "x")?;
            let y = number_arg(&args, "y")?;
            post_action(
                &client,
                &base,
                &token,
                session,
                None,
                json!({"type":"scroll","x":x,"y":y}),
            )
            .await?
        }
        "action.focus" => {
            let session = string_arg(&args, "session")?;
            let reference = string_arg(&args, "reference")?;
            post_action(
                &client,
                &base,
                &token,
                session,
                Some(reference),
                json!({"type":"focus"}),
            )
            .await?
        }
        "action.snapshot" => {
            let session = string_arg(&args, "session")?;
            post_action(
                &client,
                &base,
                &token,
                session,
                None,
                json!({"type":"snapshot"}),
            )
            .await?
        }
        "action.results" => {
            let session = string_arg(&args, "session")?;
            client
                .get(format!("{base}/v1/sessions/{session}/actions/results"))
                .bearer_auth(&token)
                .send()
                .await?
        }
        _ => return Err(anyhow::anyhow!("unknown tool: {name}")),
    };

    let status = response.status();
    if !status.is_success() {
        return Err(anyhow::anyhow!("LocalView control returned {status}"));
    }
    let content = if status == reqwest::StatusCode::NO_CONTENT {
        json!({"ok": true})
    } else {
        response.json::<Value>().await?
    };
    Ok(json!({
        "content": [{"type":"text","text":serde_json::to_string_pretty(&content)?}]
    }))
}

async fn post_action(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    session: &str,
    reference: Option<&str>,
    action: Value,
) -> Result<reqwest::Response> {
    Ok(client
        .post(format!("{base}/v1/sessions/{session}/actions"))
        .bearer_auth(token)
        .json(&json!({"reference": reference, "action": action}))
        .send()
        .await?)
}

fn string_arg<'a>(args: &'a Value, name: &str) -> Result<&'a str> {
    args.get(name)
        .and_then(Value::as_str)
        .with_context(|| format!("missing {name}"))
}

fn number_arg(args: &Value, name: &str) -> Result<f64> {
    args.get(name)
        .and_then(Value::as_f64)
        .with_context(|| format!("missing {name}"))
}

async fn read_token() -> Result<String> {
    let dir = dirs::data_local_dir()
        .context("no local data directory")?
        .join("LocalView");
    Ok(tokio::fs::read_to_string(dir.join("control.token"))
        .await?
        .trim()
        .to_owned())
}
