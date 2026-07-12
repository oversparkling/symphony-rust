export type WorkerStatus = {
  state: "stopped" | "running" | "stopping";
  started_at: string | null;
  last_error: string | null;
};

export type RunWithIssueRow = {
  id: string;
  issue_id: string;
  run_number: number;
  workspace_path: string;
  status: string;
  started_at: string | null;
  ended_at: string | null;
  error_class: string | null;
  error_message: string | null;
  worker_pid: number | null;
  session_info: string | null;
  repo_name: string | null;
  created_at: string;
  issue_identifier: string;
  issue_title: string;
  issue_state: string;
};

export type RetryWithIssueRow = {
  issue_id: string;
  run_number: number;
  due_at: string;
  error_class: string | null;
  error_message: string | null;
  created_at: string;
  issue_identifier: string;
  issue_title: string;
};

export type Overview = {
  active_runs: RunWithIssueRow[];
  retry_queue: RetryWithIssueRow[];
  recent_failures: RunWithIssueRow[];
  live_sessions: unknown[];
  worker_heartbeat: { last_beat_at: string } | null;
  rate_limits: unknown[];
  token_usage: unknown[];
};

export type IssueRow = {
  id: string;
  identifier: string;
  title: string;
  state: string;
  last_seen_at: string;
};

export type StatusSnapshot = {
  updatedAt: string;
  worker: WorkerStatus;
  overview: Overview;
  runs: RunWithIssueRow[];
  issues: IssueRow[];
};

export type RemoteCommandType =
  | "retry_now"
  | "stop_run"
  | "start_worker"
  | "stop_worker";

export type RemoteCommand = {
  id: string;
  type: RemoteCommandType;
  issueId?: string;
  runId?: string;
  createdAt: string;
  status: "pending" | "acked" | "failed";
  error?: string;
  ackedAt?: string;
};

export type CommandEnvelope = {
  commands: RemoteCommand[];
};
