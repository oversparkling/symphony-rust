//! Polls linked GitHub PRs for merge conflicts and failing CI while issues sit
//! in watch states (e.g. In Review), then moves them back to an active state so
//! Symphony can redispatch and resolve the problem.

use serde::Deserialize;
use symphony_core::{Issue, TrackerConfig};
use symphony_storage::{now_iso, PrHealthRow, Repository};
use symphony_tracker::TrackerClient;
use tokio::process::Command;
use tracing::{info, warn};

const FAILED_CHECK_CONCLUSIONS: &[&str] =
    &["FAILURE", "CANCELLED", "TIMED_OUT", "ACTION_REQUIRED"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrHealthStatus {
    Healthy,
    Conflicting,
    CiFailing,
    Closed,
    Unknown,
}

impl PrHealthStatus {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Conflicting => "conflicting",
            Self::CiFailing => "ci_failing",
            Self::Closed => "closed",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PrCheckSnapshot {
    pub pr_url: String,
    pub mergeable: String,
    pub merge_state_status: String,
    pub pr_state: String,
    pub checks_status: String,
    pub failing_checks: Vec<String>,
    pub health_status: PrHealthStatus,
    pub detail: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GhPrView {
    state: String,
    mergeable: String,
    #[serde(rename = "mergeStateStatus")]
    merge_state_status: String,
    #[serde(rename = "statusCheckRollup", default)]
    status_check_rollup: Vec<GhCheck>,
}

#[derive(Debug, Deserialize)]
struct GhCheck {
    name: String,
    #[serde(default)]
    conclusion: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

pub async fn run_pr_health_monitor<T: TrackerClient>(
    repo: &Repository,
    tracker: &T,
    tracker_config: &TrackerConfig,
) -> Result<(), PrHealthError> {
    if !tracker_config.pr_health_enabled || tracker_config.watch_states.is_empty() {
        return Ok(());
    }

    let issues = repo
        .list_issues_in_states_with_prs(&tracker_config.watch_states)
        .await?;

    for issue in issues {
        if let Err(err) = check_issue_pr_health(repo, tracker, tracker_config, &issue).await {
            warn!(
                issue = %issue.identifier,
                error = %err,
                "PR health check failed"
            );
        }
    }

    Ok(())
}

async fn check_issue_pr_health<T: TrackerClient>(
    repo: &Repository,
    tracker: &T,
    tracker_config: &TrackerConfig,
    issue: &Issue,
) -> Result<(), PrHealthError> {
    let mut snapshots = Vec::new();
    for pr_url in &issue.pr_urls {
        match check_pr_url(pr_url).await {
            Ok(snapshot) => snapshots.push(snapshot),
            Err(err) => {
                warn!(
                    issue = %issue.identifier,
                    pr_url = %pr_url,
                    error = %err,
                    "failed to check PR health"
                );
                snapshots.push(PrCheckSnapshot {
                    pr_url: pr_url.clone(),
                    mergeable: "unknown".to_string(),
                    merge_state_status: "unknown".to_string(),
                    pr_state: "unknown".to_string(),
                    checks_status: "unknown".to_string(),
                    failing_checks: Vec::new(),
                    health_status: PrHealthStatus::Unknown,
                    detail: Some(err.to_string()),
                });
            }
        }
    }

    let aggregate = aggregate_snapshots(&snapshots);
    let checked_at = now_iso();
    let auto_moved_at = maybe_auto_move(
        repo,
        tracker,
        tracker_config,
        issue,
        &aggregate,
        checked_at.clone(),
    )
    .await?;

    let failing_checks = serde_json::to_string(&aggregate.failing_checks)?;
    repo.upsert_pr_health(&PrHealthRow {
        issue_id: issue.id.clone(),
        health_status: aggregate.health_status.as_str().to_string(),
        mergeable: Some(aggregate.mergeable),
        merge_state_status: Some(aggregate.merge_state_status),
        pr_state: Some(aggregate.pr_state),
        checks_status: Some(aggregate.checks_status),
        failing_checks,
        pr_url: Some(aggregate.pr_url),
        detail: aggregate.detail,
        checked_at,
        auto_moved_at,
    })
    .await?;

    Ok(())
}

async fn maybe_auto_move<T: TrackerClient>(
    _repo: &Repository,
    tracker: &T,
    tracker_config: &TrackerConfig,
    issue: &Issue,
    aggregate: &PrCheckSnapshot,
    checked_at: String,
) -> Result<Option<String>, PrHealthError> {
    if !state_in_list(&issue.state, &tracker_config.watch_states) {
        return Ok(None);
    }

    let (should_move, target_state, reason) = match aggregate.health_status {
        PrHealthStatus::Conflicting if tracker_config.auto_move_on_conflict => (
            true,
            tracker_config.conflict_target_state.clone(),
            "merge conflict",
        ),
        PrHealthStatus::CiFailing if tracker_config.auto_move_on_ci_failure => (
            true,
            tracker_config.ci_failure_target_state.clone(),
            "failing CI checks",
        ),
        _ => return Ok(None),
    };

    if !should_move || state_matches(&issue.state, &target_state) {
        return Ok(None);
    }

    tracker
        .update_issue_state(&issue.id, &target_state)
        .await
        .map_err(PrHealthError::Tracker)?;

    let failing = if aggregate.failing_checks.is_empty() {
        String::new()
    } else {
        format!(" ({})", aggregate.failing_checks.join(", "))
    };
    let note = format!(
        "### Notes\n- Symphony auto-moved this issue to `{target_state}` because the linked PR has {reason}{failing} (checked {checked_at})."
    );
    if let Err(err) = tracker.append_workpad_note(&issue.id, &note).await {
        warn!(
            issue = %issue.identifier,
            error = %err,
            "failed to append workpad note after PR health auto-move"
        );
    }

    info!(
        issue = %issue.identifier,
        from = %issue.state,
        to = %target_state,
        reason = reason,
        "auto-moved issue after PR health check"
    );

    Ok(Some(checked_at))
}

fn aggregate_snapshots(snapshots: &[PrCheckSnapshot]) -> PrCheckSnapshot {
    let mut worst = snapshots
        .first()
        .cloned()
        .unwrap_or_else(|| PrCheckSnapshot {
            pr_url: String::new(),
            mergeable: "unknown".to_string(),
            merge_state_status: "unknown".to_string(),
            pr_state: "unknown".to_string(),
            checks_status: "unknown".to_string(),
            failing_checks: Vec::new(),
            health_status: PrHealthStatus::Unknown,
            detail: None,
        });

    for snapshot in snapshots.iter().skip(1) {
        if health_rank(&snapshot.health_status) > health_rank(&worst.health_status) {
            worst = snapshot.clone();
        }
    }

    let mut failing_checks = snapshots
        .iter()
        .flat_map(|snapshot| snapshot.failing_checks.iter().cloned())
        .collect::<Vec<_>>();
    failing_checks.sort();
    failing_checks.dedup();
    worst.failing_checks = failing_checks;
    worst
}

fn health_rank(status: &PrHealthStatus) -> u8 {
    match status {
        PrHealthStatus::Conflicting => 4,
        PrHealthStatus::CiFailing => 3,
        PrHealthStatus::Unknown => 2,
        PrHealthStatus::Closed => 1,
        PrHealthStatus::Healthy => 0,
    }
}

pub async fn check_pr_url(pr_url: &str) -> Result<PrCheckSnapshot, PrHealthError> {
    let script = format!(
        "gh pr view {} --json state,mergeable,mergeStateStatus,statusCheckRollup",
        shell_quote(pr_url)
    );
    let output = Command::new("/bin/sh")
        .arg("-lc")
        .arg(&script)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(PrHealthError::Gh(stderr.trim().to_string()));
    }

    let payload: GhPrView = serde_json::from_slice(&output.stdout)?;
    let failing_checks = failing_checks(&payload.status_check_rollup);
    let checks_status = checks_status(&payload.status_check_rollup, &failing_checks);
    let health_status = classify_health(
        &payload.state,
        &payload.mergeable,
        &payload.merge_state_status,
        &failing_checks,
    );

    Ok(PrCheckSnapshot {
        pr_url: pr_url.to_string(),
        mergeable: payload.mergeable,
        merge_state_status: payload.merge_state_status,
        pr_state: payload.state,
        checks_status,
        failing_checks,
        health_status,
        detail: None,
    })
}

fn classify_health(
    pr_state: &str,
    mergeable: &str,
    merge_state_status: &str,
    failing_checks: &[String],
) -> PrHealthStatus {
    let pr_state = pr_state.to_ascii_uppercase();
    if pr_state == "CLOSED" || pr_state == "MERGED" {
        return PrHealthStatus::Closed;
    }
    if mergeable.eq_ignore_ascii_case("CONFLICTING")
        || merge_state_status.eq_ignore_ascii_case("DIRTY")
    {
        return PrHealthStatus::Conflicting;
    }
    if mergeable.eq_ignore_ascii_case("UNKNOWN") {
        return PrHealthStatus::Unknown;
    }
    if !failing_checks.is_empty() {
        return PrHealthStatus::CiFailing;
    }
    PrHealthStatus::Healthy
}

fn failing_checks(checks: &[GhCheck]) -> Vec<String> {
    checks
        .iter()
        .filter(|check| {
            check
                .conclusion
                .as_deref()
                .is_some_and(|conclusion| FAILED_CHECK_CONCLUSIONS.contains(&conclusion))
        })
        .map(|check| check.name.clone())
        .collect()
}

fn checks_status(checks: &[GhCheck], failing: &[String]) -> String {
    if checks.is_empty() {
        return "unknown".to_string();
    }
    if !failing.is_empty() {
        return "failing".to_string();
    }
    if checks.iter().any(|check| {
        check
            .status
            .as_deref()
            .is_some_and(|status| status.eq_ignore_ascii_case("IN_PROGRESS"))
    }) {
        return "pending".to_string();
    }
    "passing".to_string()
}

fn state_in_list(state: &str, states: &[String]) -> bool {
    states
        .iter()
        .any(|candidate| state_matches(state, candidate))
}

fn state_matches(state: &str, candidate: &str) -> bool {
    state.trim().eq_ignore_ascii_case(candidate.trim())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[derive(Debug, thiserror::Error)]
pub enum PrHealthError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("gh error: {0}")]
    Gh(String),
    #[error("tracker error: {0}")]
    Tracker(#[from] symphony_tracker::TrackerError),
    #[error("storage error: {0}")]
    Storage(#[from] symphony_storage::StorageError),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_conflicting_pr() {
        let status = classify_health("OPEN", "CONFLICTING", "DIRTY", &[]);
        assert_eq!(status, PrHealthStatus::Conflicting);
    }

    #[test]
    fn classifies_failing_checks() {
        let status = classify_health("OPEN", "MERGEABLE", "CLEAN", &["CI".to_string()]);
        assert_eq!(status, PrHealthStatus::CiFailing);
    }

    #[test]
    fn classifies_closed_pr() {
        let status = classify_health("MERGED", "MERGEABLE", "CLEAN", &[]);
        assert_eq!(status, PrHealthStatus::Closed);
    }

    #[test]
    fn picks_worst_snapshot() {
        let snapshots = vec![
            PrCheckSnapshot {
                pr_url: "https://github.com/a/b/pull/1".to_string(),
                mergeable: "MERGEABLE".to_string(),
                merge_state_status: "CLEAN".to_string(),
                pr_state: "OPEN".to_string(),
                checks_status: "passing".to_string(),
                failing_checks: vec![],
                health_status: PrHealthStatus::Healthy,
                detail: None,
            },
            PrCheckSnapshot {
                pr_url: "https://github.com/a/b/pull/2".to_string(),
                mergeable: "CONFLICTING".to_string(),
                merge_state_status: "DIRTY".to_string(),
                pr_state: "OPEN".to_string(),
                checks_status: "passing".to_string(),
                failing_checks: vec![],
                health_status: PrHealthStatus::Conflicting,
                detail: None,
            },
        ];
        let aggregate = aggregate_snapshots(&snapshots);
        assert_eq!(aggregate.health_status, PrHealthStatus::Conflicting);
    }

    #[test]
    fn parses_gh_payload() {
        let raw = r#"{
            "state": "OPEN",
            "mergeable": "MERGEABLE",
            "mergeStateStatus": "CLEAN",
            "statusCheckRollup": [
                {"name": "CI", "conclusion": "FAILURE", "status": "COMPLETED"},
                {"name": "lint", "conclusion": "SUCCESS", "status": "COMPLETED"}
            ]
        }"#;
        let payload: GhPrView = serde_json::from_str(raw).unwrap();
        let failing = failing_checks(&payload.status_check_rollup);
        assert_eq!(failing, vec!["CI".to_string()]);
        assert_eq!(
            classify_health(
                &payload.state,
                &payload.mergeable,
                &payload.merge_state_status,
                &failing
            ),
            PrHealthStatus::CiFailing
        );
    }
}
