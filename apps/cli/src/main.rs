#![forbid(unsafe_code)]

use std::path::PathBuf;
use anyhow::{Context,Result};
use clap::{Parser,Subcommand};
use localview_protocol::{Health,Session,SessionId};
use reqwest::Client;

#[derive(Parser)]
#[command(name="localview",version,about="AI-native localhost visual runtime")]
struct Cli { #[arg(long,env="LOCALVIEW_CONTROL",default_value="http://127.0.0.1:45454")] control:String, #[command(subcommand)] command:Command }
#[derive(Subcommand)] enum Command { Status, Sessions, Show{session:SessionId}, Pause, Resume }

#[tokio::main]
async fn main()->Result<()> {
    let cli=Cli::parse(); let client=Client::new();
    match cli.command {
        Command::Status=>{let h:Health=client.get(format!("{}/health",cli.control)).send().await?.error_for_status()?.json().await?; println!("LocalView {} — {} — {} session(s){}",h.version,h.status,h.sessions,if h.paused{" — PAUSED"}else{""});}
        Command::Sessions=>{let sessions:Vec<Session>=authed(&client,&cli.control,"/v1/sessions").await?.json().await?; for s in sessions{println!("{}  {:<8}  {}:{}  {:<20}  {}",s.id,format!("{:?}",s.status),s.endpoint.host,s.endpoint.port,s.classification.framework.unwrap_or_else(||"generic".into()),s.project.display_name);}}
        Command::Show{session}=>{let s:Session=authed(&client,&cli.control,&format!("/v1/sessions/{session}")).await?.json().await?; println!("{}",serde_json::to_string_pretty(&s)?);}
        Command::Pause=>{post(&client,&cli.control,"/v1/runtime/pause").await?; println!("LocalView discovery paused");}
        Command::Resume=>{post(&client,&cli.control,"/v1/runtime/resume").await?; println!("LocalView discovery resumed");}
    }
    Ok(())
}
async fn authed(client:&Client,base:&str,path:&str)->Result<reqwest::Response>{let token=read_token().await?;Ok(client.get(format!("{base}{path}")).bearer_auth(token).send().await?.error_for_status()?)}
async fn post(client:&Client,base:&str,path:&str)->Result<()> {let token=read_token().await?;client.post(format!("{base}{path}")).bearer_auth(token).send().await?.error_for_status()?;Ok(())}
async fn read_token()->Result<String>{let path=state_dir()?.join("control.token");Ok(tokio::fs::read_to_string(&path).await.with_context(||format!("cannot read {} — is localview-daemon running?",path.display()))?.trim().to_owned())}
fn state_dir()->Result<PathBuf>{dirs::data_local_dir().map(|p|p.join("LocalView")).context("no local data directory")}
