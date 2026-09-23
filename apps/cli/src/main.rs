#![forbid(unsafe_code)]

mod headless;

use std::{net::Ipv4Addr, path::PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use localview_live_bridge::{BridgeAction, BridgeActionKind, BridgeActionResult, ObserverEvent};
use localview_protocol::{Health, Session, SessionId};
use reqwest::{Client, Response};
use serde::Serialize;
use serde_json::Value;

#[derive(Parser)]
#[command(name = "localview", version, about = "AI-native localhost visual runtime")]
struct Cli {
    #[arg(long, env = "LOCALVIEW_CONTROL", default_value = "http://127.0.0.1:45454")]
    control: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Headless(headless::HeadlessArgs),
    Status,
    Sessions,
    Show { session: SessionId },
    ProjectState { session: Option<SessionId> },
    Pause,
    Resume,
    Observer {
        session: SessionId,
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    Analyze { session: Option<SessionId> },
    Diagnose { session: Option<SessionId> },
    PerformanceLite { session: Option<SessionId> },
    CaptureSettle { session: Option<SessionId> },
    ActionCorrelation { session: SessionId, action_id: String },
    SourceMapResolve {
        session: SessionId,
        generated_file: String,
        generated_line: u32,
        generated_column: u32,
    },
    Verify { session: Option<SessionId> },
    Coverage { session: Option<SessionId> },
    Proof { session: Option<SessionId> },
    Evidence {
        session: Option<SessionId>,
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    EvidenceGet { evidence_id: String },
    EvidenceTrace { evidence_id: String },
    ProofStaleness { evidence_id: String },
    Snapshot { session: SessionId },
    ActionResults {
        session: SessionId,
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
}

#[derive(Debug, Serialize)]
struct QueueActionRequest {
    reference: Option<String>,
    action: BridgeActionKind,
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut cli = Cli::parse();
    cli.control = canonical_control_origin(&cli.control)?;
    let client = control_client()?;

    match cli.command {
        Command::Headless(args) => {
            let token = match read_token().await {
                Ok(token) => token,
                Err(error) => {
                    eprintln!("LocalView headless infrastructure error: {error:#}");
                    std::process::exit(headless::EXIT_INFRASTRUCTURE);
                }
            };
            let code = match headless::run(&client, &cli.control, &token, args).await {
                Ok(code) => code,
                Err(error) => {
                    eprintln!("LocalView headless infrastructure error: {error:#}");
                    headless::EXIT_INFRASTRUCTURE
                }
            };
            std::process::exit(code);
        }
        Command::Status => {
            let health: Health = client
                .get(format!("{}/health", cli.control))
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            println!(
                "LocalView {} — {} — {} session(s){}",
                health.version,
                health.status,
                health.sessions,
                if health.paused { " — PAUSED" } else { "" }
            );
        }
        Command::Sessions => {
            let sessions: Vec<Session> = authed_get(&client, &cli.control, "/v1/sessions")
                .await?
                .json()
                .await?;
            for session in sessions {
                println!(
                    "{}  {:<12}  {}:{}  {:<20}  {}",
                    session.id,
                    format!("{:?}", session.status),
                    session.endpoint.host,
                    session.endpoint.port,
                    session
                        .classification
                        .framework
                        .unwrap_or_else(|| "generic".into()),
                    session.project.display_name
                );
            }
        }
        Command::Show { session } => {
            let value: Session = authed_get(
                &client,
                &cli.control,
                &format!("/v1/sessions/{session}"),
            )
            .await?
            .json()
            .await?;
            print_json(&value)?;
        }
        Command::ProjectState { session } => {
            print_session_endpoint(&client, &cli.control, session, "project-state").await?;
        }
        Command::Pause => {
            authed_post_empty(&client, &cli.control, "/v1/runtime/pause").await?;
            println!("LocalView discovery paused");
        }
        Command::Resume => {
            authed_post_empty(&client, &cli.control, "/v1/runtime/resume").await?;
            println!("LocalView discovery resumed");
        }
        Command::Observer { session, limit } => {
            let events: Vec<ObserverEvent> = authed_get(
                &client,
                &cli.control,
                &format!("/v1/sessions/{session}/observer/recent"),
            )
            .await?
            .json()
            .await?;
            let start = events.len().saturating_sub(limit);
            print_json(&events[start..])?;
        }
        Command::Analyze { session } => {
            print_session_endpoint(&client, &cli.control, session, "analysis").await?;
        }
        Command::Diagnose { session } => {
            print_session_endpoint(&client, &cli.control, session, "diagnose").await?;
        }
        Command::PerformanceLite { session } => {
            print_session_endpoint(&client, &cli.control, session, "performance-lite").await?;
        }
        Command::CaptureSettle { session } => {
            print_session_endpoint(&client, &cli.control, session, "capture-settle").await?;
        }
        Command::ActionCorrelation { session, action_id } => {
            print_path(
                &client,
                &cli.control,
                &format!("/v1/sessions/{session}/actions/{action_id}/correlation"),
            )
            .await?;
        }
        Command::SourceMapResolve {
            session,
            generated_file,
            generated_line,
            generated_column,
        } => {
            let value: Value = authed_post_json(
                &client,
                &cli.control,
                &format!("/v1/sessions/{session}/source-map/resolve"),
                &serde_json::json!({
                    "generated_file": generated_file,
                    "generated_line": generated_line,
                    "generated_column": generated_column,
                }),
            )
            .await?
            .json()
            .await?;
            print_json(&value)?;
        }
        Command::Verify { session } => {
            print_session_endpoint(&client, &cli.control, session, "verify").await?;
        }
        Command::Coverage { session } => {
            print_session_endpoint(&client, &cli.control, session, "coverage").await?;
        }
        Command::Proof { session } => {
            let session = resolve_session(&client, &cli.control, session).await?;
            let value: Value = authed_post_empty_response(
                &client,
                &cli.control,
                &format!("/v1/sessions/{session}/proof"),
            )
            .await?
            .json()
            .await?;
            print_json(&value)?;
        }
        Command::Evidence { session, limit } => {
            let session = resolve_session(&client, &cli.control, session).await?;
            let evidence: Vec<Value> = authed_get(
                &client,
                &cli.control,
                &format!("/v1/sessions/{session}/evidence/recent"),
            )
            .await?
            .json()
            .await?;
            let start = evidence.len().saturating_sub(limit);
            print_json(&evidence[start..])?;
        }
        Command::EvidenceGet { evidence_id } => {
            print_path(&client, &cli.control, &format!("/v1/evidence/{evidence_id}")).await?;
        }
        Command::EvidenceTrace { evidence_id } => {
            print_path(
                &client,
                &cli.control,
                &format!("/v1/evidence/{evidence_id}/trace"),
            )
            .await?;
        }
        Command::ProofStaleness { evidence_id } => {
            print_path(
                &client,
                &cli.control,
                &format!("/v1/proof/{evidence_id}/staleness"),
            )
            .await?;
        }
        Command::Snapshot { session } => {
            queue_action(
                &client,
                &cli.control,
                session,
                None,
                BridgeActionKind::Snapshot,
            )
            .await?;
        }
        Command::ActionResults { session, limit } => {
            let results: Vec<BridgeActionResult> = authed_get(
                &client,
                &cli.control,
                &format!("/v1/sessions/{session}/actions/results"),
            )
            .await?
            .json()
            .await?;
            let start = results.len().saturating_sub(limit);
            print_json(&results[start..])?;
        }
    }
    Ok(())
}

async fn print_session_endpoint(
    client: &Client,
    base: &str,
    requested: Option<SessionId>,
    endpoint: &str,
) -> Result<()> {
    let session = resolve_session(client, base, requested).await?;
    print_path(client, base, &format!("/v1/sessions/{session}/{endpoint}")).await
}

async fn print_path(client: &Client, base: &str, path: &str) -> Result<()> {
    let value: Value = authed_get(client, base, path).await?.json().await?;
    print_json(&value)
}

async fn resolve_session(
    client: &Client,
    base: &str,
    requested: Option<SessionId>,
) -> Result<SessionId> {
    if let Some(session) = requested {
        return Ok(session);
    }
    let sessions: Vec<Session> = authed_get(client, base, "/v1/sessions").await?.json().await?;
    match sessions.as_slice() {
        [] => Err(anyhow::anyhow!("no LocalView sessions are active")),
        [session] => Ok(session.id),
        _ => Err(anyhow::anyhow!(
            "multiple LocalView sessions are active; pass a session id explicitly"
        )),
    }
}

async fn queue_action(
    client: &Client,
    base: &str,
    session: SessionId,
    reference: Option<String>,
    action: BridgeActionKind,
) -> Result<()> {
    let request = QueueActionRequest { reference, action };
    let queued: BridgeAction = authed_post_json(
        client,
        base,
        &format!("/v1/sessions/{session}/actions"),
        &request,
    )
    .await?
    .json()
    .await?;
    print_json(&queued)
}

async fn authed_get(client: &Client, base: &str, path: &str) -> Result<Response> {
    let token = read_token().await?;
    Ok(client
        .get(format!("{base}{path}"))
        .bearer_auth(token)
        .send()
        .await?
        .error_for_status()?)
}

async fn authed_post_empty(client: &Client, base: &str, path: &str) -> Result<()> {
    authed_post_empty_response(client, base, path).await?;
    Ok(())
}

async fn authed_post_empty_response(client: &Client, base: &str, path: &str) -> Result<Response> {
    let token = read_token().await?;
    Ok(client
        .post(format!("{base}{path}"))
        .bearer_auth(token)
        .send()
        .await?
        .error_for_status()?)
}

async fn authed_post_json<T: Serialize + ?Sized>(
    client: &Client,
    base: &str,
    path: &str,
    body: &T,
) -> Result<Response> {
    let token = read_token().await?;
    Ok(client
        .post(format!("{base}{path}"))
        .bearer_auth(token)
        .json(body)
        .send()
        .await?
        .error_for_status()?)
}

fn canonical_control_origin(raw: &str) -> Result<String> {
    let url = reqwest::Url::parse(raw.trim())
        .context("LocalView control origin is invalid")?;
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

fn control_client() -> Result<Client> {
    Ok(Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}

async fn read_token() -> Result<String> {
    let path = state_dir()?.join("control.token");
    Ok(tokio::fs::read_to_string(&path)
        .await
        .with_context(|| format!("cannot read {} — is localview-daemon running?", path.display()))?
        .trim()
        .to_owned())
}

fn state_dir() -> Result<PathBuf> {
    dirs::data_local_dir()
        .map(|path| path.join("LocalView"))
        .context("no local data directory")
}

fn print_json<T: Serialize + ?Sized>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
        time::Duration,
    };

    fn read_http_headers(stream: &mut std::net::TcpStream) -> String {
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
    fn cli_does_not_advertise_legacy_consequential_dom_actions() {
        use clap::Parser as _;

        assert!(Cli::try_parse_from(["localview", "snapshot", "550e8400-e29b-41d4-a716-446655440000"]).is_ok());
        assert!(Cli::try_parse_from(["localview", "performance-lite"]).is_ok());
        assert!(Cli::try_parse_from(["localview", "capture-settle"]).is_ok());
        assert!(Cli::try_parse_from([
            "localview",
            "action-correlation",
            "550e8400-e29b-41d4-a716-446655440000",
            "11111111-2222-3333-4444-555555555555",
        ]).is_ok());
        assert!(Cli::try_parse_from([
            "localview",
            "source-map-resolve",
            "550e8400-e29b-41d4-a716-446655440000",
            "dist/app.js",
            "42",
            "7",
        ]).is_ok());
        for forbidden in ["click", "type", "key", "scroll", "focus"] {
            let parsed = Cli::try_parse_from(["localview", forbidden]);
            assert!(
                parsed.is_err(),
                "legacy consequential command must stay unavailable: {forbidden}"
            );
        }
    }

    #[test]
    fn control_origin_is_exact_loopback_http_origin() {
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
            "http://user:secret@127.0.0.1:45454",
            "http://127.0.0.1:45454/v1",
            "http://127.0.0.1:45454/?token=secret",
            "http://127.0.0.1:45454/#fragment",
        ] {
            let error = canonical_control_origin(rejected).expect_err(rejected);
            assert!(!error.to_string().contains("secret"));
        }
    }

    #[tokio::test]
    async fn control_client_never_redirects_authorization_to_another_origin() {
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
