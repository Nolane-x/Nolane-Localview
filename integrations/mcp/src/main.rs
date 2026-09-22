#![forbid(unsafe_code)]

use std::{
    io::{self, BufRead, Write},
    net::Ipv4Addr,
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
        json!({"name":"session.performance_lite","description":"Read the bounded live performance-lite packet for one session","inputSchema":session_schema()}),
        json!({"name":"session.capture_settle","description":"Read the current bounded capture-settle decision for one session without capturing pixels","inputSchema":session_schema()}),
        json!({"name":"action.correlation","description":"Read bounded action→request→UI-response correlation for one exact action id","inputSchema":{"type":"object","properties":{"session":{"type":"string"},"actionId":{"type":"string"}},"required":["session","actionId"]}}),
        json!({"name":"source.resolve","description":"Resolve one generated JS/CSS position through a project-contained source map","inputSchema":{"type":"object","properties":{"session":{"type":"string"},"generatedFile":{"type":"string"},"generatedLine":{"type":"integer","minimum":1},"generatedColumn":{"type":"integer","minimum":0}},"required":["session","generatedFile","generatedLine","generatedColumn"]}}),
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
    let raw_base = std::env::var("LOCALVIEW_CONTROL")
        .unwrap_or_else(|_| "http://127.0.0.1:45454".into());
    let base = canonical_control_origin(&raw_base)?;
    let client = control_client()?;
    let token = read_token().await?;

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
        "session.performance_lite" => session_get(&client, &base, &token, &args, "performance-lite").await?,
        "session.capture_settle" => session_get(&client, &base, &token, &args, "capture-settle").await?,
        "action.correlation" => {
            let session = string_arg(&args, "session")?;
            let action_id = string_arg(&args, "actionId")?;
            authed_get(
                &client,
                &base,
                &token,
                &format!("/v1/sessions/{session}/actions/{action_id}/correlation"),
            )
            .await?
        },
        "source.resolve" => {
            let session = string_arg(&args, "session")?;
            let generated_file = string_arg(&args, "generatedFile")?;
            let generated_line = u32_arg(&args, "generatedLine")?;
            let generated_column = u32_arg(&args, "generatedColumn")?;
            authed_post_json(
                &client,
                &base,
                &token,
                &format!("/v1/sessions/{session}/source-map/resolve"),
                &json!({
                    "generated_file": generated_file,
                    "generated_line": generated_line,
                    "generated_column": generated_column,
                }),
            )
            .await?
        }
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

async fn authed_post_json(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    path: &str,
    body: &Value,
) -> Result<reqwest::Response> {
    Ok(client
        .post(format!("{base}{path}"))
        .bearer_auth(token)
        .json(body)
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

fn u32_arg(args: &Value, name: &str) -> Result<u32> {
    let value = args
        .get(name)
        .and_then(Value::as_u64)
        .with_context(|| format!("missing or invalid {name}"))?;
    u32::try_from(value).with_context(|| format!("{name} exceeds u32 range"))
}

fn canonical_control_origin(raw: &str) -> Result<String> {
    let url = reqwest::Url::parse(raw.trim()).context("LocalView control origin is invalid")?;
    if url.scheme() != "http"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        anyhow::bail!("LocalView control origin must be a bare loopback HTTP origin");
    }

    if url.port_or_known_default() != Some(45454) {
        anyhow::bail!("LocalView control origin must use the daemon control port");
    }

    let host = url
        .host_str()
        .context("LocalView control origin is missing a host")?;
    if host.parse::<Ipv4Addr>().ok() != Some(Ipv4Addr::LOCALHOST) {
        anyhow::bail!("LocalView control origin must use the daemon IPv4 loopback address");
    }

    Ok(url.origin().ascii_serialization())
}

fn control_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
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

    fn read_http_headers(stream: &mut std::net::TcpStream) -> String {
        use std::io::Read as _;

        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .expect("set source read timeout");
        let mut request = Vec::new();
        let mut chunk = [0_u8; 1024];
        while request.len() < 8192 {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => {
                    request.extend_from_slice(&chunk[..read]);
                    if request.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    break;
                }
                Err(error) => panic!("read source request: {error}"),
            }
        }
        String::from_utf8_lossy(&request).into_owned()
    }

    fn assert_bearer_header(request: &str, expected: &str) {
        let value = request
            .lines()
            .filter_map(|line| line.split_once(':'))
            .find_map(|(name, value)| {
                name.eq_ignore_ascii_case("authorization")
                    .then_some(value.trim())
            })
            .expect("source request must carry authorization");
        assert_eq!(value, expected);
    }

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

    #[test]
    fn advertised_action_tools_never_expose_legacy_consequential_mutations() {
        let names = tool_definitions()
            .into_iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str).map(str::to_owned))
            .collect::<Vec<_>>();
        assert!(names.contains(&"action.snapshot".to_owned()));
        assert!(names.contains(&"session.performance_lite".to_owned()));
        assert!(names.contains(&"session.capture_settle".to_owned()));
        assert!(names.contains(&"action.correlation".to_owned()));
        assert!(names.contains(&"source.resolve".to_owned()));
        for forbidden in [
            "action.click",
            "action.type",
            "action.key",
            "action.scroll",
            "action.focus",
        ] {
            assert!(!names.contains(&forbidden.to_owned()), "{forbidden}");
        }
    }

    #[test]
    fn control_origin_is_exact_loopback_http_origin_and_errors_do_not_echo_secrets() {
        for allowed in ["http://127.0.0.1:45454", "http://127.0.0.1:45454/"] {
            assert!(canonical_control_origin(allowed).is_ok(), "{allowed}");
        }
        for rejected in [
            "https://127.0.0.1:45454",
            "http://localhost:45454",
            "http://[::1]:45454",
            "http://127.0.0.1:45455",
            "http://127.0.0.1",
            "http://localhost",
            "http://127.0.0.2:45454",
            "http://10.0.0.2:45454",
            "http://example.com:45454",
            "http://user:CONTROL_SECRET@127.0.0.1:45454",
            "http://127.0.0.1:45454/v1",
            "http://127.0.0.1:45454/?token=CONTROL_SECRET",
            "http://127.0.0.1:45454/#CONTROL_SECRET",
        ] {
            let error = canonical_control_origin(rejected).expect_err(rejected);
            assert!(!error.to_string().contains("CONTROL_SECRET"));
        }
    }

    #[tokio::test]
    async fn control_client_never_redirects_bearer_authority() {
        use std::{
            io::Write,
            net::TcpListener,
            thread,
        };

        let target = TcpListener::bind(("127.0.0.1", 0)).expect("bind redirect target");
        target.set_nonblocking(true).unwrap();
        let target_port = target.local_addr().unwrap().port();

        let source = TcpListener::bind(("127.0.0.1", 0)).expect("bind redirect source");
        let source_port = source.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut stream, _) = source.accept().expect("accept source request");
            let request = read_http_headers(&mut stream);
            let response = format!(
                "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{target_port}/steal\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            stream.write_all(response.as_bytes()).unwrap();
            request
        });

        let client = control_client().unwrap();
        let response = client
            .get(format!("http://127.0.0.1:{source_port}/v1/sessions"))
            .bearer_auth("CONTROL_TOKEN_MUST_NOT_LEAK")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::FOUND);
        let source_request = server.join().unwrap();
        assert_bearer_header(&source_request, "Bearer CONTROL_TOKEN_MUST_NOT_LEAK");

        thread::sleep(Duration::from_millis(100));
        assert!(
            matches!(target.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
            "redirect target must receive no request at all"
        );
    }
}
