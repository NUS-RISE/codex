# AURA research fork

This repository retains the OpenAI Codex source and license and adds experimental
request-level usage observations to `codex exec --json` for AURA trajectory and
cost analysis. It does not replace the ordinary `turn.completed` aggregate.

The patch is confined to `codex-rs/exec`:

- Enable experimental raw events when starting the exec thread.
- Emit `model.usage` for raw response completions containing usage, retaining the
  response ID and `usage_source: "raw_response_completed"`.
- When direct usage is absent, emit a fallback only if every `last` usage field
  matches the cumulative increment and counts are nonnegative and consistent.
  These observations use `usage_source: "token_usage_last_reconciled"`, a local
  request index and a null response ID.
- Suppress repeated token-count updates and fallback duplicates of direct usage.
- Retain input, cached input, cache-write input, output and reasoning-output
  counts. Cached reads are included in input; reasoning is included in output.
  Consumers must avoid double counting these subsets.

These are provider usage observations, not invoices or per-command cost
attribution. Unknown or inconsistent request increments are not estimated by
splitting aggregate totals. This fork is separate from AURA's official-binary
native-rollout usage recovery wrapper; experiment manifests identify the client
and instrumentation actually used.

From `codex-rs`, the focused unit suite is:

```sh
just test -p codex-exec --lib
```

Follow the upstream build guidance and `AGENTS.md` for development. The original
OpenAI repository remains the local `origin`; `nus-rise` is the publication
remote. GitHub Actions are initially disabled for this research copy so upstream
CI/release workflows are not automatically launched by publication.
