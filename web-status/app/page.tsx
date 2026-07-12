"use client";

import { useCallback, useEffect, useMemo, useState } from "react";
import type {
  RemoteCommandType,
  RunWithIssueRow,
  StatusSnapshot,
} from "@/lib/types";

const TOKEN_KEY = "symphony_status_token";
const STALE_MS = 60_000;

type Tab = "overview" | "runs" | "issues";

function authHeaders(token: string): HeadersInit {
  return {
    Authorization: `Bearer ${token}`,
    "Content-Type": "application/json",
  };
}

function formatAge(iso: string | null | undefined): string {
  if (!iso) return "—";
  const ms = Date.now() - new Date(iso).getTime();
  if (!Number.isFinite(ms) || ms < 0) return "just now";
  const sec = Math.floor(ms / 1000);
  if (sec < 60) return `${sec}s ago`;
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m ago`;
  const hr = Math.floor(min / 60);
  return `${hr}h ago`;
}

function statusPill(status: string) {
  const value = status.toLowerCase();
  if (value === "running" || value === "success") return "ok";
  if (value === "failure" || value === "timeout") return "bad";
  if (value === "cancelled" || value === "pending") return "warn";
  return "";
}

export default function HomePage() {
  const [token, setToken] = useState<string | null>(null);
  const [draftToken, setDraftToken] = useState("");
  const [unlockError, setUnlockError] = useState<string | null>(null);
  const [snapshot, setSnapshot] = useState<StatusSnapshot | null>(null);
  const [empty, setEmpty] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>("overview");
  const [now, setNow] = useState(Date.now());

  useEffect(() => {
    const saved = sessionStorage.getItem(TOKEN_KEY);
    if (saved) setToken(saved);
  }, []);

  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 5000);
    return () => window.clearInterval(id);
  }, []);

  const refresh = useCallback(async (activeToken: string) => {
    try {
      const res = await fetch("/api/status", {
        headers: authHeaders(activeToken),
        cache: "no-store",
      });
      if (res.status === 401) {
        sessionStorage.removeItem(TOKEN_KEY);
        setToken(null);
        setUnlockError("Token rejected. Try again.");
        return;
      }
      if (!res.ok) {
        setLoadError(`Status fetch failed (${res.status})`);
        return;
      }
      const data = await res.json();
      if (data?.empty) {
        setSnapshot(null);
        setEmpty(true);
      } else {
        setSnapshot(data as StatusSnapshot);
        setEmpty(false);
      }
      setLoadError(null);
    } catch (err) {
      setLoadError(err instanceof Error ? err.message : "Network error");
    }
  }, []);

  useEffect(() => {
    if (!token) return;
    void refresh(token);
    const id = window.setInterval(() => void refresh(token), 10_000);
    return () => window.clearInterval(id);
  }, [token, refresh]);

  async function unlock(event: React.FormEvent) {
    event.preventDefault();
    const next = draftToken.trim();
    if (!next) {
      setUnlockError("Enter the status token");
      return;
    }
    const res = await fetch("/api/status", {
      headers: authHeaders(next),
      cache: "no-store",
    });
    if (res.status === 401) {
      setUnlockError("Invalid token");
      return;
    }
    if (!res.ok) {
      setUnlockError(`Could not unlock (${res.status})`);
      return;
    }
    sessionStorage.setItem(TOKEN_KEY, next);
    setToken(next);
    setUnlockError(null);
  }

  async function sendCommand(
    type: RemoteCommandType,
    extra: { issueId?: string; runId?: string } = {},
  ) {
    if (!token) return;
    const key = `${type}:${extra.issueId ?? extra.runId ?? "worker"}`;
    setBusy(key);
    try {
      const res = await fetch("/api/commands", {
        method: "POST",
        headers: authHeaders(token),
        body: JSON.stringify({ type, ...extra }),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        setLoadError(body.error ?? `Command failed (${res.status})`);
      } else {
        setLoadError(null);
      }
    } catch (err) {
      setLoadError(err instanceof Error ? err.message : "Command failed");
    } finally {
      setBusy(null);
    }
  }

  const stale = useMemo(() => {
    if (!snapshot?.updatedAt) return true;
    return now - new Date(snapshot.updatedAt).getTime() > STALE_MS;
  }, [snapshot, now]);

  if (!token) {
    return (
      <div className="unlock">
        <form className="unlock-card" onSubmit={unlock}>
          <h1>Symphony</h1>
          <p>Enter your private status token to view and control the worker.</p>
          {unlockError ? <div className="error">{unlockError}</div> : null}
          <input
            type="password"
            autoComplete="current-password"
            placeholder="STATUS_TOKEN"
            value={draftToken}
            onChange={(e) => setDraftToken(e.currentTarget.value)}
          />
          <button className="primary" type="submit" style={{ width: "100%" }}>
            Unlock
          </button>
        </form>
      </div>
    );
  }

  const workerState = snapshot?.worker.state ?? "stopped";
  const workerPill =
    workerState === "running" ? "ok" : workerState === "stopping" ? "warn" : "bad";

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          <h1>Symphony</h1>
          <span className={`pill ${workerPill}`}>{workerState}</span>
        </div>
        <div className="meta" style={{ marginTop: 6 }}>
          Updated {formatAge(snapshot?.updatedAt)}
          {snapshot?.overview.worker_heartbeat?.last_beat_at
            ? ` · beat ${formatAge(snapshot.overview.worker_heartbeat.last_beat_at)}`
            : ""}
        </div>
      </header>

      {stale ? (
        <div className="banner">
          Mac looks offline or Symphony isn&apos;t syncing. Commands will wait until
          the desktop app is running.
        </div>
      ) : null}
      {loadError ? <div className="banner">{loadError}</div> : null}
      {empty ? (
        <div className="banner">
          No snapshot yet. Open Symphony on your Mac and set the Web Status URL +
          token.
        </div>
      ) : null}

      <div className="card">
        <h2>Worker</h2>
        <div className="row">
          <div>
            <div className="title">Desktop worker</div>
            <div className="sub">
              {snapshot?.worker.last_error
                ? snapshot.worker.last_error
                : workerState === "running"
                  ? "Dispatching Linear issues"
                  : "Not running"}
            </div>
          </div>
        </div>
        <div className="actions">
          <button
            className="primary"
            disabled={busy !== null || workerState === "running"}
            onClick={() => void sendCommand("start_worker")}
          >
            Start
          </button>
          <button
            className="danger"
            disabled={busy !== null || workerState === "stopped"}
            onClick={() => void sendCommand("stop_worker")}
          >
            Stop
          </button>
          <button disabled={busy !== null} onClick={() => token && void refresh(token)}>
            Refresh
          </button>
        </div>
      </div>

      <div className="tabs">
        <button
          className={tab === "overview" ? "active" : ""}
          onClick={() => setTab("overview")}
        >
          Overview
        </button>
        <button
          className={tab === "runs" ? "active" : ""}
          onClick={() => setTab("runs")}
        >
          Runs
        </button>
        <button
          className={tab === "issues" ? "active" : ""}
          onClick={() => setTab("issues")}
        >
          Issues
        </button>
      </div>

      {tab === "overview" ? (
        <>
          <Section
            title="Active runs"
            rows={snapshot?.overview.active_runs ?? []}
            busy={busy}
            onRetry={(issueId) => void sendCommand("retry_now", { issueId })}
            onStop={(runId) => void sendCommand("stop_run", { runId })}
          />
          <Section
            title="Retry queue"
            rows={(snapshot?.overview.retry_queue ?? []).map((item) => ({
              id: `${item.issue_id}:${item.run_number}`,
              issue_id: item.issue_id,
              run_number: item.run_number,
              workspace_path: "",
              status: "pending",
              started_at: null,
              ended_at: null,
              error_class: item.error_class,
              error_message: item.error_message,
              worker_pid: null,
              session_info: null,
              repo_name: null,
              created_at: item.created_at,
              issue_identifier: item.issue_identifier,
              issue_title: item.issue_title,
              issue_state: "retry",
            }))}
            busy={busy}
            onRetry={(issueId) => void sendCommand("retry_now", { issueId })}
          />
          <Section
            title="Recent failures"
            rows={snapshot?.overview.recent_failures ?? []}
            busy={busy}
            onRetry={(issueId) => void sendCommand("retry_now", { issueId })}
          />
        </>
      ) : null}

      {tab === "runs" ? (
        <Section
          title="Recent runs"
          rows={snapshot?.runs ?? []}
          busy={busy}
          onRetry={(issueId) => void sendCommand("retry_now", { issueId })}
          onStop={(runId) => void sendCommand("stop_run", { runId })}
        />
      ) : null}

      {tab === "issues" ? (
        <div className="stack">
          {(snapshot?.issues ?? []).length === 0 ? (
            <div className="card empty">No issues in the latest snapshot.</div>
          ) : (
            (snapshot?.issues ?? []).map((issue) => (
              <div className="card" key={issue.id}>
                <div className="row">
                  <div>
                    <div className="title">
                      {issue.identifier} · {issue.title}
                    </div>
                    <div className="sub">
                      {issue.state} · seen {formatAge(issue.last_seen_at)}
                    </div>
                  </div>
                  <span className="pill">{issue.state}</span>
                </div>
                <div className="actions">
                  <button
                    className="primary"
                    disabled={busy !== null}
                    onClick={() =>
                      void sendCommand("retry_now", { issueId: issue.id })
                    }
                  >
                    Retry
                  </button>
                </div>
              </div>
            ))
          )}
        </div>
      ) : null}
    </div>
  );
}

function Section({
  title,
  rows,
  busy,
  onRetry,
  onStop,
}: {
  title: string;
  rows: RunWithIssueRow[];
  busy: string | null;
  onRetry?: (issueId: string) => void;
  onStop?: (runId: string) => void;
}) {
  return (
    <div className="card">
      <h2>{title}</h2>
      {rows.length === 0 ? (
        <div className="empty">Nothing here.</div>
      ) : (
        <div className="stack">
          {rows.map((run) => (
            <div key={run.id} style={{ padding: "8px 0", borderTop: "1px solid var(--line)" }}>
              <div className="row">
                <div>
                  <div className="title">
                    {run.issue_identifier} · Run #{run.run_number}
                  </div>
                  <div className="sub">{run.issue_title}</div>
                  <div className="sub">
                    {run.repo_name ? `${run.repo_name} · ` : ""}
                    {formatAge(run.ended_at ?? run.started_at ?? run.created_at)}
                    {run.error_message ? ` · ${run.error_message}` : ""}
                  </div>
                </div>
                <span className={`pill ${statusPill(run.status)}`}>{run.status}</span>
              </div>
              <div className="actions">
                {onRetry ? (
                  <button
                    className="primary"
                    disabled={busy !== null}
                    onClick={() => onRetry(run.issue_id)}
                  >
                    Retry
                  </button>
                ) : null}
                {onStop && run.status === "running" ? (
                  <button
                    className="danger"
                    disabled={busy !== null}
                    onClick={() => onStop(run.id)}
                  >
                    Stop
                  </button>
                ) : null}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
