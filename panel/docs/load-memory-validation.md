# Load And Memory Validation

This document defines the acceptance procedure for the target deployment envelope:

- `1 vCPU`
- `512 MB RAM`
- `10 GB disk`

The goal is to prove that the panel can run on cheap hosts without hidden unbounded buffers, runaway logs, or opaque memory growth.

## Scope

This validation covers the Rust panel backend.

It does not prove Xray, Hysteria2, WireGuard, or future sidecar memory usage by itself. Node/runtime processes must be measured separately and then combined with the panel budget before release.

## Required API Contract

The panel must expose:

- `GET /api/system/overview`
- `GET /api/system/resource-budget`

`/api/system/resource-budget` must include:

- configured memory budget;
- current panel process RSS;
- process RSS percentage of budget;
- current panel process CPU snapshot;
- all bounded runtime collection sizes;
- configured limit for each collection;
- worst status across process RSS and tracked buffers;
- human-readable recommendations.

The endpoint must not expose secrets, tokens, private keys, SSH credentials, route material, or full subscription credentials.

## Local Smoke Run

Build and test the backend:

```bash
cargo test
```

Run the panel with explicit low-memory defaults:

```bash
HYDRA_BIND_ADDR=127.0.0.1:8000 \
HYDRA_BOOTSTRAP_ADMIN_USERNAME=admin \
HYDRA_BOOTSTRAP_ADMIN_PASSWORD='change-this-before-real-use' \
cargo run -p panel-app
```

Authenticate with the admin API, then request:

```bash
curl -fsS http://127.0.0.1:8000/api/system/resource-budget \
  -H "Authorization: Bearer $TOKEN"
```

Acceptance criteria:

- `status` is `ok` or explainably `warning` during development builds;
- `process_memory_percent_of_budget` stays below `80` in normal idle state;
- no active `panel_memory_budget` alert is present in normal idle state;
- no item has `status = "over_limit"`;
- recommendations do not report required compaction;
- repeated calls do not increase tracked buffer sizes by themselves.

## Constrained Host Acceptance

On a real or emulated `1 vCPU / 512 MB RAM / 10 GB disk` host:

1. Install the release build, not a debug build.
2. Start the panel with the default `runtime_limits.memory_budget_mb = 512`.
3. Log in as admin.
4. Poll `/api/system/overview` and `/api/system/resource-budget` for at least 10 minutes.
5. Exercise these flows:
   - login success and failed login;
   - audit history query;
   - system logs query;
   - node list/readiness query;
   - subscription render query;
   - installer plan dry-run;
   - provisioning plan dry-run.
6. Confirm process RSS returns to a stable range after burst actions.

Fail the validation if:

- panel RSS exceeds the configured budget;
- any runtime collection grows without a configured bound;
- an API call returns unbounded logs/history by default;
- repeated polling grows server-side state;
- secrets appear in logs, reports, or resource-budget output;
- the panel becomes unresponsive under idle polling on the target host.

## Buffer Discipline

Current bounded collections must remain compacted by default:

- security audit events;
- login IP counters;
- admin sessions;
- system logs;
- system alert events;
- core apply history;
- Telegram delivery events;
- user activity events;
- subscription devices;
- subscription sessions;
- subscription usage points;
- subscription enforcement actions;
- node sync/apply/bootstrap histories;
- node provisioning tasks/events/submissions;
- panel installer jobs.

Compaction rules:

- active operational state is kept before terminal state;
- recent records are kept before old records;
- terminal or old records are dropped first;
- persistence must not re-expand compacted buffers.

## Release Gate

Before starting frontend release work, run one backend audit pass against this document and confirm:

- all buffer limits are documented in `panel-config`;
- all API list endpoints have explicit limits;
- installer/provisioning jobs are bounded;
- process RSS is visible through `/api/system/resource-budget`;
- target deployment assumptions are still `1 vCPU / 512 MB RAM / 10 GB disk`.
