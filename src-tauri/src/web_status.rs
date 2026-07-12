//! Push local Symphony status to the personal web dashboard and pull remote
//! commands (retry / stop / start-stop worker) queued from the phone UI.

use crate::{
    load_settings_from_disk, validate_workflow_settings, worker_start_config, AppState,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tracing::{debug, warn};

const SYNC_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusSnapshot<'a> {
    updated_at: String,
    worker: symphony_worker::WorkerStatus,
    overview: &'a symphony_storage::Overview,
    runs: &'a [symphony_storage::RunWithIssueRow],
    issues: &'a [SlimIssue],
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SlimIssue {
    id: String,
    identifier: String,
    title: String,
    state: String,
    last_seen_at: String,
}

#[derive(Debug, Deserialize)]
struct CommandsResponse {
    commands: Vec<RemoteCommand>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteCommand {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    issue_id: Option<String>,
    #[serde(default)]
    run_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct AckBody {
    acks: Vec<AckItem>,
}

#[derive(Debug, Serialize)]
struct AckItem {
    id: String,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

pub fn spawn(handle: AppHandle) {
    tauri::async_runtime::spawn(async move {
        // Small delay so settings/DB are ready after startup.
        tokio::time::sleep(Duration::from_secs(3)).await;
        loop {
            if let Err(err) = sync_once(&handle).await {
                debug!(target: "symphony", %err, "web status sync skipped");
            }
            tokio::time::sleep(SYNC_INTERVAL).await;
        }
    });
}

async fn sync_once(handle: &AppHandle) -> Result<(), String> {
    let state = handle
        .try_state::<AppState>()
        .ok_or_else(|| "app state unavailable".to_string())?;
    let settings = load_settings_from_disk(state.inner()).await?;
    let url = settings
        .web_status_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let token = settings
        .web_status_token
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let (Some(base_url), Some(token)) = (url, token) else {
        return Ok(());
    };
    let base = base_url.trim_end_matches('/');

    pull_and_run_commands(state.inner(), base, token).await?;
    push_status(state.inner(), base, token).await?;
    Ok(())
}

async fn push_status(state: &AppState, base: &str, token: &str) -> Result<(), String> {
    let overview = state
        .repo
        .overview()
        .await
        .map_err(|err| err.to_string())?;
    let runs = state
        .repo
        .list_runs(50)
        .await
        .map_err(|err| err.to_string())?;
    let issues = state
        .repo
        .list_issues(50)
        .await
        .map_err(|err| err.to_string())?;
    let slim_issues: Vec<SlimIssue> = issues
        .into_iter()
        .map(|issue| SlimIssue {
            id: issue.id,
            identifier: issue.identifier,
            title: issue.title,
            state: issue.state,
            last_seen_at: issue.last_seen_at,
        })
        .collect();
    let worker = state.worker.status().await;
    let snapshot = StatusSnapshot {
        updated_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        worker,
        overview: &overview,
        runs: &runs,
        issues: &slim_issues,
    };

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{base}/api/status"))
        .bearer_auth(token)
        .json(&snapshot)
        .send()
        .await
        .map_err(|err| err.to_string())?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("status push failed: {status} {body}"));
    }
    Ok(())
}

async fn pull_and_run_commands(state: &AppState, base: &str, token: &str) -> Result<(), String> {
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{base}/api/commands?pending=1"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|err| err.to_string())?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("command pull failed: {status} {body}"));
    }
    let payload: CommandsResponse = response.json().await.map_err(|err| err.to_string())?;
    if payload.commands.is_empty() {
        return Ok(());
    }

    let mut acks = Vec::with_capacity(payload.commands.len());
    for command in payload.commands {
        match execute_command(state, &command).await {
            Ok(()) => acks.push(AckItem {
                id: command.id,
                ok: true,
                error: None,
            }),
            Err(error) => {
                warn!(
                    target: "symphony",
                    command_id = %command.id,
                    %error,
                    "web status command failed"
                );
                acks.push(AckItem {
                    id: command.id,
                    ok: false,
                    error: Some(error),
                });
            }
        }
    }

    let ack_response = client
        .post(format!("{base}/api/commands/ack"))
        .bearer_auth(token)
        .json(&AckBody { acks })
        .send()
        .await
        .map_err(|err| err.to_string())?;
    if !ack_response.status().is_success() {
        let status = ack_response.status();
        let body = ack_response.text().await.unwrap_or_default();
        return Err(format!("command ack failed: {status} {body}"));
    }
    Ok(())
}

async fn execute_command(state: &AppState, command: &RemoteCommand) -> Result<(), String> {
    match command.kind.as_str() {
        "retry_now" => {
            let issue_id = command
                .issue_id
                .as_deref()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| "retry_now missing issueId".to_string())?;
            let queued = state
                .worker
                .trigger_retry_now(issue_id)
                .await
                .map_err(|err| err.to_string())?;
            if !queued {
                return Err("retry not queued (issue may already be active)".to_string());
            }
            Ok(())
        }
        "stop_run" => {
            let run_id = command
                .run_id
                .as_deref()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| "stop_run missing runId".to_string())?;
            state
                .worker
                .stop_run(run_id)
                .await
                .map_err(|err| err.to_string())
        }
        "start_worker" => {
            let settings = load_settings_from_disk(state).await?;
            if let Some(error) = validate_workflow_settings(&settings) {
                return Err(error);
            }
            state
                .worker
                .start(worker_start_config(state, &settings))
                .await
                .map_err(|err| err.to_string())?;
            Ok(())
        }
        "stop_worker" => {
            let _ = state.worker.stop().await;
            Ok(())
        }
        other => Err(format!("unknown command type: {other}")),
    }
}
