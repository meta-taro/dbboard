# dbboard-anthropic

Anthropic Messages API provider for dbboard's optional AI layer
(ADR-0023). Implements the
[`AiProvider`](../dbboard-ai/src/provider.rs) trait from `dbboard-ai`
by POSTing to `/v1/messages` over `reqwest`.

This is a Stage 1 implementation: `explain` and `suggest_sql`, both
single-shot. Streaming, function calling, conversation history,
persisted keys, and the multi-provider switcher are Stage 2 concerns
recorded in ADR-0023 §9.

## Configuration

Stage 1 wiring is env-var-only and lives in `apps/dbboard`. The crate
itself accepts an [`AnthropicConfig`] / [`AnthropicProvider::new`]
constructor that the binary populates from these variables:

| Variable                       | Required | Default              |
|--------------------------------|----------|----------------------|
| `DBBOARD_ANTHROPIC_API_KEY`    | yes      | —                    |
| `DBBOARD_ANTHROPIC_MODEL`      | no       | `claude-sonnet-4-6`  |

When `DBBOARD_ANTHROPIC_API_KEY` is absent or empty, the binary does
not construct the provider; the AI menu entry and the panel both
hide — graceful degradation by absence (ADR-0023 Decision 11). When
present, the desktop UI's worker thread routes
`Command::AiExplain { sql, dialect }` and
`Command::AiSuggest { prompt, dialect, schema }` to this provider via
`tokio::runtime::block_on(provider.explain | suggest_sql)` and surfaces
the result as `Reply::AiResponded { text, tokens_in, tokens_out }` or
`Reply::AiFailed { error: AiError }`.

The API key is held privately on the provider struct and never appears
in `Debug` output, log lines, or `AiError` messages.

## Error classification

Per ADR-0023 §8 and issue 0005's acceptance list:

| Outcome                                | Variant                |
|----------------------------------------|------------------------|
| Empty / whitespace key or model        | `AiError::Configuration` (at construction) |
| Network / TLS / timeout                | `AiError::Network`     |
| HTTP 4xx (incl. 401 auth, 429 rate-limit) | `AiError::Provider` |
| HTTP 5xx                               | `AiError::Provider`    |
| Malformed JSON response                | `AiError::Provider`    |
| Reserved for Stage 2 budget enforcement | `AiError::Quota`      |
| Mid-flight cancel                      | `AiError::Cancelled`   |

A runtime 401 is intentionally **not** re-raised as `Configuration` —
the Stage 1 design trusts construction-time validation and treats any
runtime rejection as a provider-side decision.

## Deferrals (Stage 2)

Recorded in ADR-0023 §9; out of scope for this crate:

- Streaming responses (`AiProvider::streaming` accessor + chunked
  replies)
- Function calling / tool use
- `ai-providers.toml` + OS keychain (today: env-var-only, mirroring the
  Phase 1 `DBBOARD_TURSO_PATH` → ADR-0013 `connections.toml` evolution
  path)
- Recording AI calls in the query history (ADR-0017)
- Multi-provider switcher UI
- Full DDL snapshots (today: `list_tables()` only)
- Token budget meter + cancel button

## Testing

`cargo test -p dbboard-anthropic` drives every test against a
`wiremock` mock server bound to loopback — no live network calls, no
required env vars. The live round-trip test (gated behind
`DBBOARD_ANTHROPIC_API_KEY`) is deferred to a follow-up issue.
