#![forbid(unsafe_code)]

use anyhow::{Context,Result};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use std::io::{self,BufRead,Write};

#[derive(Debug,Deserialize)]struct RpcRequest{jsonrpc:String,id:Option<Value>,method:String,#[serde(default)]params:Value}
#[derive(Debug,Serialize)]struct RpcResponse{jsonrpc:&'static str,id:Option<Value>,#[serde(skip_serializing_if="Option::is_none")]result:Option<Value>,#[serde(skip_serializing_if="Option::is_none")]error:Option<Value>}

#[tokio::main]
async fn main()->Result<()> {
    let stdin=io::stdin();let mut stdout=io::stdout();
    for line in stdin.lock().lines(){let line=line?;if line.trim().is_empty(){continue;}let req:RpcRequest=match serde_json::from_str(&line){Ok(v)=>v,Err(e)=>{writeln!(stdout,"{}",serde_json::to_string(&RpcResponse{jsonrpc:"2.0",id:None,result:None,error:Some(json!({"code":-32700,"message":e.to_string()}))})?)?;stdout.flush()?;continue;}};
        let response=handle(req).await;writeln!(stdout,"{}",serde_json::to_string(&response)?)?;stdout.flush()?;
    }Ok(())
}

async fn handle(req:RpcRequest)->RpcResponse{
    let id=req.id.clone();let result=match req.method.as_str(){
        "initialize"=>Ok(json!({"protocolVersion":"2025-06-18","serverInfo":{"name":"localview","version":env!("CARGO_PKG_VERSION")},"capabilities":{"tools":{}}})),
        "tools/list"=>Ok(json!({"tools":tool_definitions()})),
        "tools/call"=>call_tool(&req.params).await,
        _=>Err(anyhow::anyhow!("method not found: {}",req.method)),
    };
    match result{Ok(v)=>RpcResponse{jsonrpc:"2.0",id,result:Some(v),error:None},Err(e)=>RpcResponse{jsonrpc:"2.0",id,result:None,error:Some(json!({"code":-32000,"message":e.to_string()}))}}
}

fn tool_definitions()->Vec<Value>{vec![
    json!({"name":"session.list","description":"List detected LocalView sessions","inputSchema":{"type":"object","properties":{}}}),
    json!({"name":"session.inspect","description":"Inspect one session","inputSchema":{"type":"object","properties":{"id":{"type":"string"}},"required":["id"]}}),
    json!({"name":"runtime.pause","description":"Pause localhost discovery","inputSchema":{"type":"object","properties":{}}}),
    json!({"name":"runtime.resume","description":"Resume localhost discovery","inputSchema":{"type":"object","properties":{}}}),
    json!({"name":"events.recent","description":"Return compact recent runtime events","inputSchema":{"type":"object","properties":{}}}),
]}

async fn call_tool(params:&Value)->Result<Value>{let name=params.get("name").and_then(Value::as_str).context("missing tool name")?;let args=params.get("arguments").cloned().unwrap_or_else(||json!({}));let base=std::env::var("LOCALVIEW_CONTROL").unwrap_or_else(|_|"http://127.0.0.1:45454".into());let token=read_token().await?;let client=reqwest::Client::new();let response=match name{
    "session.list"=>client.get(format!("{base}/v1/sessions")).bearer_auth(&token).send().await?,
    "session.inspect"=>{let id=args.get("id").and_then(Value::as_str).context("missing id")?;client.get(format!("{base}/v1/sessions/{id}")).bearer_auth(&token).send().await?},
    "runtime.pause"=>client.post(format!("{base}/v1/runtime/pause")).bearer_auth(&token).send().await?,
    "runtime.resume"=>client.post(format!("{base}/v1/runtime/resume")).bearer_auth(&token).send().await?,
    "events.recent"=>client.get(format!("{base}/v1/events/recent")).bearer_auth(&token).send().await?,
    _=>return Err(anyhow::anyhow!("unknown tool: {name}")),};let status=response.status();if !status.is_success(){return Err(anyhow::anyhow!("LocalView control returned {status}"));}let content=if status==reqwest::StatusCode::NO_CONTENT{json!({"ok":true})}else{response.json::<Value>().await?};Ok(json!({"content":[{"type":"text","text":serde_json::to_string_pretty(&content)?}]}))}

async fn read_token()->Result<String>{let dir=dirs::data_local_dir().context("no local data directory")?.join("LocalView");Ok(tokio::fs::read_to_string(dir.join("control.token")).await?.trim().to_owned())}
