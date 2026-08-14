# CX58 Agent Production Incident and Fixes, 2026-08-14

## S3 TLS crash after container recreation

### Symptoms and evidence

- GMR failed to reach `http://cx58-agent:3050`.
- The container briefly appeared on the external `ispredict` network and disappeared
  again during its restart loop.
- Docker reported `ExitCode=139`, `OOMKilled=false`, and `Status=restarting`.
- Agent logs consistently stopped at `Initializing S3 with rust-s3...`.

The network disappearance was secondary. Exit code 139 means the agent received
`SIGSEGV`; Docker then recreated the container and its network endpoint.

### Cause and fix

`rust-s3` used its `tokio-native-tls` feature. The crash occurred in the Alpine
native TLS/OpenSSL runtime path during S3 initialization. The dependency was changed
to `tokio-rustls-tls`, removing that native OpenSSL ABI dependency from S3 TLS.

Runtime validation passed:

- PostgreSQL connection and migrations;
- S3 initialization and connection verification;
- application-state initialization and HTTP server startup;
- database and S3 health checks;
- zero container restarts during the stability probe.

## Intermittent `Show prev description` failure

### Symptoms and evidence

After a successful comparison, the first `Show prev description` request sometimes
failed while resolving a storage path. Repeating the same action succeeded.

Production DB inspection established that failing UUID
`019d29da-6f5c-75e9-b436-08a183abfad7` was the `Branch` node `Room 11`, not an
`ImageLeaf`. The UUID was not stored in `chat_session.prev_leaf` or
`chat_session.next_leaf`.

### Cause and fix

The orchestrator LLM sometimes copied `object_id` into `parameters.reports.prev`.
Because both object IDs and report IDs are valid UUIDs, syntax validation accepted
the value. `DescriptionAgent` then attempted to load the branch as an image report.

When resolved report context exists, `Orchestrator::parse_decision` now treats
`current_report_id` and `previous_report_id` as authoritative. Conflicting LLM report
parameters are ignored and logged. LLM values remain a fallback only while report
context is unresolved.

A regression test reproduces the object-ID/report-ID mix-up and verifies that the
worker receives the resolved report ID without a leaked second report.

Validation results:

- `cargo test --lib` with `SQLX_OFFLINE=true`: 62 passed, 0 failed;
- 12 S3 integration tests ignored by design;
- the user confirmed the `Show prev description` fix in cloud.

## Images and delivery boundary

- Initial TLS image: `alsh/cx58a:rustls-20260814`.
- Final image with both fixes: `alsh/cx58a:prev-description-fix-20260814`.
- Local `alsh/cx58a:latest` points to the final image.
- Final local image ID:
  `sha256:8faa89cb2d9ed605ac5298b0f0a8339accfd302f67c76b247c116df6bed1ac86`.
- The final compiled binary was checked for the authoritative-context guard.
- No push was performed because the remote server was unavailable.
