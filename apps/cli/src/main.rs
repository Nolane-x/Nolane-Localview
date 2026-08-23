#![forbid(unsafe_code)]

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use localview_live_bridge::{BridgeAction, BridgeActionKind, BridgeActionResult, ObserverEvent};
use localview_protocol::{Health, Session, SessionId};
use reqwest::{Client, Response};
use serde::Serialize;

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
    Status,
    Sessions,
    Show { session: SessionId },
    Pause,
    Resume,
    Observer {
        session: SessionId,
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    Click {
        session: SessionId,
        reference: String,
    },
    Type {
        session: SessionId,
        reference: String,
        text: String,
        #[arg(long)]
        clear_first: bool,
    },
    Key {
        session: SessionId,
        key: String,
        #[arg(long)]
        reference: Option<String>,
        #[arg(long = "modifier")]
        modifiers: Vec<String>,
    },
    Scroll {
        session: SessionId,
        x: f64,
        y: f64,
    },
    Focus {
        session: SessionId,
        reference: String,
    },
    Snapshot {
        session: SessionId,
    },
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
    let cli = Cli::parse();
    let client = Client::new();

    match cli.command {
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
        Command::Click { session, reference } => {
            queue_action(
                &client,
                &cli.control,
                session,
                Some(reference),
                BridgeActionKind::Click,
            )
            .await?;
        }
        Command::Type {
            session,
            reference,
            text,
            clear_first,
        } => {
            queue_action(
                &client,
                &cli.control,
                session,
                Some(reference),
                BridgeActionKind::TypeText { text, clear_first },
            )
            .await?;
        }
        Command::Key {
            session,
            key,
            reference,
            modifiers,
        } => {
            queue_action(
                &client,
                &cli.control,
                session,
                reference,
                BridgeActionKind::Key { key, modifiers },
            )
            .await?;
        }
        Command::Scroll { session, x, y } => {
            queue_action(
                &client,
                &cli.control,
                session,
                None,
                BridgeActionKind::Scroll { x, y },
            )
            .await?;
        }
        Command::Focus { session, reference } => {
            queue_action(
                &client,
                &cli.control,
                session,
                Some(reference),
                BridgeActionKind::Focus,
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
    let token = read_token().await?;
    client
        .post(format!("{base}{path}"))
        .bearer_auth(token)
        .send()
        .await?
        .error_for_status()?;
    Ok(())
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
