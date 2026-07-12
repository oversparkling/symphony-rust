create table pr_health (
  issue_id text primary key references issues(id) on delete cascade,
  health_status text not null default 'unknown',
  mergeable text,
  merge_state_status text,
  pr_state text,
  checks_status text,
  failing_checks text not null default '[]',
  pr_url text,
  detail text,
  checked_at text not null,
  auto_moved_at text
);
