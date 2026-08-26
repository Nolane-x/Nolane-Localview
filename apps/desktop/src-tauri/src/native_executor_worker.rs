#![forbid(unsafe_code)]

use std::time::Duration;

use localview_live_bridge::{NativeExecutorRequest, NativeExecutorResult};
use localview_protocol::{Session, SessionId};

use crate::{control_client, read_token, visual_capture};

const NATIVE_EXECUTOR_POLL_INTERVAL: Duration = Duration::from_millis(120);
const MAX_SESSIONS_PER_POLL: usize = 16;
const RESULT_POST_ATTEMPTS: usize = 3;
const RESULT_POST_RETRY_DELAY: Duration = Duration::from_millis(80);

pub(crate) fn spawn(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut session_cursor = 0usize;
        loop {
            let _ = poll_once(&app, &mut session_cursor).await;
            tokio::time::sleep(NATIVE_EXECUTOR_POLL_INTERVAL).await;
        }
    });
}

async fn poll_once(app: &tauri::AppHandle, session_cursor: &mut usize) -> Result<(), String> {
    let token = read_token().await?;
    let client = control_client()?;
    let sessions = client
        .get("http://127.0.0.1:45454/v1/sessions")
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json::<Vec<Session>>()
        .await
        .map_err(|error| error.to_string())?;

    if sessions.is_empty() {
        *session_cursor = 0;
        return Ok(());
    }

    let count = sessions.len().min(MAX_SESSIONS_PER_POLL);
    let start = *session_cursor % sessions.len();
    let session_ids = (0..count)
        .map(|offset| sessions[(start + offset) % sessions.len()].id)
        .collect::<Vec<_>>();
    *session_cursor = (start + count) % sessions.len();

    for session_id in session_ids {
        let _ = poll_session(app, &client, &token, session_id).await;
    }
    Ok(())
}

async fn poll_session(
    app: &tauri::AppHandle,
    client: &reqwest::Client,
    token: &str,
    session_id: SessionId,
) -> Result<(), String> {
    let requests = client
        .get(format!(
            "http://127.0.0.1:45454/v1/sessions/{session_id}/native-executor"
        ))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json::<Vec<NativeExecutorRequest>>()
        .await
        .map_err(|error| error.to_string())?;

    for request in requests {
        let state = app.state::<visual_capture::VisualCaptureState>();
        let result = visual_capture::execute_native_visual_packet(app.clone(), state, request).await;
        post_result(client, token, session_id, &result).await;
    }
    Ok(())
}

async fn post_result(
    client: &reqwest::Client,
    token: &str,
    session_id: SessionId,
    result: &NativeExecutorResult,
) {
    for attempt in 0..RESULT_POST_ATTEMPTS {
        let accepted = client
            .post(format!(
                "http://127.0.0.1:45454/v1/sessions/{session_id}/native-executor/results"
            ))
            .bearer_auth(token)
            .json(result)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .is_ok();
        if accepted {
            return;
        }
        if attempt + 1 < RESULT_POST_ATTEMPTS {
            tokio::time::sleep(RESULT_POST_RETRY_DELAY).await;
        }
    }
}
