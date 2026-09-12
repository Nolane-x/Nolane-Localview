#![forbid(unsafe_code)]

use std::{
    io::{self, BufRead, Write},
    time::Duration,
};

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

fn session_schema() -> Value {
    json!({"type":"object","properties":{"session":{"type":"string"}},"required":["session"]})
}

fn id_schema() -> Value {
    json!({"type":"object","properties":{"id":{"type":"string"}},"required":["id"]})
}

fn tool_definitions() -> Vec<Value> {
    vec![
        json!({"name":"session.list","description":"List detected LocalView sessions","inputSchema":{"type":"object","properties":{}}}),
        json!({"name":"session.inspect","description":"Inspect one LocalView session","inputSchema":{"type":"object","properties":{"id":{"type":"string"}},"required":["id"]}}),
        json!({"name":"session.project_state","description":"Read Git branch, commit, dirty files and working-tree identity without mutating the repository","inputSchema":session_schema()}),
        json!({"name":"session.analysis","description":"Analyze retained live console, network and performance evidence","inputSchema":session_schema()}),
        json!({"name":"session.diagnose","description":"Return evidence-first findings, uncertainty and recommended next checks","inputSchema":session_schema()}),
        json!({"name":"session.verify","description":"Verify the current UI using revision-bound fresh evidence; inconclusive is never promoted to pass","inputSchema":session_schema()}),
        json!({"name":"session.coverage","description":"Report strict current-target coverage without inventing a project denominator","inputSchema":session_schema()}),
        json!({"name":"session.proof","description":"Create and persist a content-addressed verification proof for the current session","inputSchema":session_schema()}),
        json!({"name":"evidence.recent","description":"Read recent content-addressed evidence objects for one session","inputSchema":session_schema()}),
        json!({"name":"evidence.get","description":"Read one evidence object by id","inputSchema":id_schema()}),
        json!({"name":"evidence.trace","description":"Trace provenance parents for one evidence object","inputSchema":id_schema()}),
        json!({"name":"proof.staleness","description":"Check whether a stored proof is stale against the current working-tree revision","inputSchema":id_schema()}),
        json!({"name":"runtime.pause","description":"Pause localhost discovery","inputSchema":{"type":"object","properties":{}}}),
        json!({"name":"runtime.resume","description":"Resume localhost discovery","inputSchema":{"type":"object","properties":{}}}),
        json!({"name":"events.recent","description":"Return recent daemon runtime events","inputSchema":{"type":"object","properties":{}}}),
        json!({"name":"observer.recent","description":"Read recent in-page observer events for one session","inputSchema":session_schema()}),
        json!({"name":"page.snapshot","description":"Return a completed privacy-bounded semantic, ARIA, style and geometry snapshot from the active LocalView page bridge","inputSchema":session_schema()}),
        json!({"name":"page.inspect","description":"Return one element from a fresh semantic snapshot using its stable LocalView reference","inputSchema":{"type":"object","properties":{"session":{"type":"string"},"reference":{"type":"string"}},"required":["session","reference"]}}),
        json!({"name":"action.click","description":"Queue a click against a stable LocalView element reference","inputSchema":{"type":"object","properties":{"session":{"type":"string"},"reference":{"type":"string"}},"required":["session","reference"]}}),
        json!({"name":"action.type","description":"Queue text input against a stable element reference","inputSchema":{"type":"object","properties":{"session":{"type":"string"},"reference":{"type":"string"},"text":{"type":"string"},"clear_first":{"type":"boolean","default":false}},"required":["session","reference","text"]}}),
        json!({"name":"action.key","description":"Queue a keyboard event","inputSchema":{"type":"object","properties":{"session":{"type":"string"},"reference":{"type":"string"},"key":{"type":"string"},"modifiers":{"type":"array","items":{"type":"string"}}},"required":["session","key"]}}),
        json!({"name":"action.scroll","description":"Queue a deterministic scroll offset","inputSchema":{"type":"object","properties":{"session":{"type":"string"},"x":{"type":"number"},"y":{"type":"number"}},"required":["session","x","y"]}}),
        json!({"name":"action.focus","description":"Queue focus for a stable element reference","inputSchema":{"type":"object","properties":{"session":{"type":"string"},"reference":{"type":"string"}},"required":["session","reference"]}}),
        json!({"name":"action.snapshot","description":"Queue a privacy-scrubbed semantic/layout snapshot without waiting for completion","inputSchema":session_schema()}),
        json!({"name":"action.results","description":"Read recent page action results","inputSchema":session_schema()}),
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

    if name == "page.snapshot" {
        let session = string_arg(&args, "session")?;
        let payload = fresh_page_snapshot(&client, &base, &token, session).await?;
        return tool_content(payload);
    }
    if name == "page.inspect" {
        let session = string_arg(&args, "session")?;
        let reference = string_arg(&args, "reference")?;
        let snapshot = fresh_page_snapshot(&client, &base, &token, session).await?;
        let node = find_semantic_node(&snapshot, reference)
            .cloned()
            .with_context(|| format!("element reference not found in fresh snapshot: {reference}"))?;
        return tool_content(json!({
            "reference": reference,
            "version": snapshot.get("version"),
            "route": snapshot.get("route"),
            "viewport": snapshot.get("viewport"),
            "node": node,
        }));
    }

    let response = match name {
        "session.list" => authed_get(&client, &base, &token, "/v1/sessions").await?,
        "session.inspect" => {
            let id = string_arg(&args, "id")?;
            authed_get(&client, &base, &token, &format!("/v1/sessions/{id}")).await?
        }
        "session.project_state" => session_get(&client, &base, &token, &args, "project-state").await?,
        "session.analysis" => session_get(&client, &base, &token, &args, "analysis").await?,
        "session.diagnose" => session_get(&client, &base, &token, &args, "diagnose").await?,
        "session.verify" => session_get(&client, &base, &token, &args, "verify").await?,
        "session.coverage" => session_get(&client, &base, &token, &args, "coverage").await?,
        "session.proof" => session_post(&client, &base, &token, &args, "proof").await?,
        "evidence.recent" => {
            let session = string_arg(&args, "session")?;
            authed_get(
                &client,
                &base,
                &token,
                &format!("/v1/sessions/{session}/evidence/recent"),
            )
            .await?
        }
        "evidence.get" => {
            let id = string_arg(&args, "id")?;
            authed_get(&client, &base, &token, &format!("/v1/evidence/{id}")).await?
        }
        "evidence.trace" => {
            let id = string_arg(&args, "id")?;
            authed_get(
                &client,
                &base,
                &token,
                &format!("/v1/evidence/{id}/trace"),
            )
            .await?
        }
        "proof.staleness" => {
            let id = string_arg(&args, "id")?;
            authed_get(
                &client,
                &base,
                &token,
                &format!("/v1/proof/{id}/staleness"),
            )
            .await?
        }
        "runtime.pause" => authed_post(&client, &base, &token, "/v1/runtime/pause").await?,
        "runtime.resume" => authed_post(&client, &base, &token, "/v1/runtime/resume").await?,
        "events.recent" => authed_get(&client, &base, &token, "/v1/events/recent").await?,
        "observer.recent" => session_get(&client, &base, &token, &args, "observer/recent").await?,
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
        "action.results" => session_get(&client, &base, &token, &args, "actions/results").await?,
        _ => return Err(anyhow::anyhow!("unknown tool: {name}")),
    };

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("LocalView control returned {status}: {body}"));
    }
    let content = if status == reqwest::StatusCode::NO_CONTENT {
        json!({"ok": true})
    } else {
        response.json::<Value>().await?
    };
    tool_content(content)
}

async fn fresh_page_snapshot(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    session: &str,
) -> Result<Value> {
    execute_page_action(
        client,
        base,
        token,
        session,
        None,
        json!({"type":"snapshot"}),
    )
    .await
}

async fn execute_page_action(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    session: &str,
    reference: Option<&str>,
    action: Value,
) -> Result<Value> {
    let queued = post_action(client, base, token, session, reference, action)
        .await?
        .error_for_status()?
        .json::<Value>()
        .await?;
    let action_id = queued
        .get("id")
        .and_then(Value::as_str)
        .context("LocalView action response did not contain an id")?
        .to_owned();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);

    loop {
        let response = authed_get(
            client,
            base,
            token,
            &format!("/v1/sessions/{session}/actions/results"),
        )
        .await?
        .error_for_status()?;
        let results = response.json::<Vec<Value>>().await?;
        if let Some(result) = results.iter().rev().find(|result| {
            result.get("action_id").and_then(Value::as_str) == Some(action_id.as_str())
        }) {
            if result.get("ok").and_then(Value::as_bool) == Some(true) {
                return Ok(result.get("payload").cloned().unwrap_or(Value::Null));
            }
            let message = result
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("LocalView page action failed");
            return Err(anyhow::anyhow!(message.to_owned()));
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(anyhow::anyhow!(
                "LocalView page bridge did not complete action {action_id} within 2 seconds"
            ));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn find_semantic_node<'a>(snapshot: &'a Value, reference: &str) -> Option<&'a Value> {
    fn visit<'a>(node: &'a Value, reference: &str) -> Option<&'a Value> {
        if node.get("ref").and_then(Value::as_str) == Some(reference) {
            return Some(node);
        }
        node.get("children")
            .and_then(Value::as_array)
            .and_then(|children| children.iter().find_map(|child| visit(child, reference)))
    }

    snapshot.get("semantic_tree").and_then(|root| visit(root, reference))
}

fn tool_content(content: Value) -> Result<Value> {
    Ok(json!({
        "content": [{"type":"text","text":serde_json::to_string_pretty(&content)?}]
    }))
}

async fn session_get(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    args: &Value,
    suffix: &str,
) -> Result<reqwest::Response> {
    let session = string_arg(args, "session")?;
    authed_get(
        client,
        base,
        token,
        &format!("/v1/sessions/{session}/{suffix}"),
    )
    .await
}

async fn session_post(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    args: &Value,
    suffix: &str,
) -> Result<reqwest::Response> {
    let session = string_arg(args, "session")?;
    authed_post(
        client,
        base,
        token,
        &format!("/v1/sessions/{session}/{suffix}"),
    )
    .await
}

async fn authed_get(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    path: &str,
) -> Result<reqwest::Response> {
    Ok(client
        .get(format!("{base}{path}"))
        .bearer_auth(token)
        .send()
        .await?)
}

async fn authed_post(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    path: &str,
) -> Result<reqwest::Response> {
    Ok(client
        .post(format!("{base}{path}"))
        .bearer_auth(token)
        .send()
        .await?)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_node_lookup_walks_nested_snapshot() {
        let snapshot = json!({
            "semantic_tree": {
                "ref": "@root",
                "children": [{
                    "ref": "@section",
                    "children": [{"ref": "@save", "role": "button", "children": []}]
                }]
            }
        });
        assert_eq!(
            find_semantic_node(&snapshot, "@save")
                .and_then(|node| node.get("role"))
                .and_then(Value::as_str),
            Some("button")
        );
        assert!(find_semantic_node(&snapshot, "@missing").is_none());
    }
}
