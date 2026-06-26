//! Background HTTP worker bridging the synchronous egui UI to the
//! loopback server.
//!
//! egui runs the UI on one thread and expects `update` to return
//! promptly, so blocking network calls cannot happen there. This worker
//! owns a dedicated thread with its own single-threaded tokio runtime,
//! drains [`Command`]s off the channel, performs the matching HTTP call
//! with `reqwest`, and posts a [`Reply`] back — waking the UI thread via
//! [`egui::Context::request_repaint`] so it drains the reply promptly.
//!
//! A per-request transport failure (server unreachable) maps to a
//! `Connection` error reply, so the UI shows it rather than deadlocking.
//! [`report_fatal`] covers the rarer case where the worker cannot even
//! build its runtime or HTTP client: it answers every command with that
//! error so the UI still makes progress.
//!
//! [`Command::SwitchConnection`] (ADR-0020) is NOT translated to HTTP —
//! the swap is an in-process operation on the local server's
//! `AppState`, not a wire concept. The worker delegates it to an
//! injected [`ConnectionSwitcher`] supplied by the binary, which holds
//! everything the swap needs (the live `AppState`, the connection
//! store, secrets, and a runtime handle for `build_adapter`).

use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, PoisonError, RwLock};
use std::thread;

use dbboard_ai::{AiError, AiProvider, ExplainRequest, SuggestRequest};
use dbboard_core::DbError;
use eframe::egui;

/// Shared, atomically-swappable slot for the active AI provider
/// (ADR-0025). The binary constructs the slot once at startup
/// (initialised by the precedence chain `env > ai-providers.toml > None`)
/// and clones the `Arc` into two places:
///
/// * the UI / worker side, which reads it on every `AiExplain` /
///   `AiSuggest` dispatch through [`run_worker`];
/// * the [`AiProviderSwitcher`] implementation, which writes it on every
///   `SwitchAiProvider` command.
///
/// `std::sync::RwLock` rather than `tokio::sync::RwLock` because the
/// worker's runtime is `current_thread` and the slot is held only across
/// a `clone()` of the inner `Arc` — no `.await` happens under the lock.
/// Lock contention is microscopic (writes only on user-driven swaps).
pub type AiProviderSlot = Arc<RwLock<Option<Arc<dyn AiProvider>>>>;

use crate::client::{self, HttpRequest};
use crate::{Command, Reply};

/// Bridge from a `Command::SwitchConnection { id }` to the actual swap.
/// The worker calls [`Self::switch`] from its dedicated thread (so the
/// impl may block) and turns the result into a [`Reply`]. The desktop
/// binary supplies the production impl in `apps/dbboard`; tests inject
/// a stub.
///
/// The trait is intentionally narrow — given an `id`, either return
/// `Ok(())` to signal a clean swap, or return a `DbError` whose
/// category reflects the failure (typically `Connection` or
/// `Capability`). The UI does not need to know how the swap happened.
pub trait ConnectionSwitcher: Send + Sync + 'static {
    /// Swap the live adapter to the connection named `id`.
    ///
    /// # Errors
    ///
    /// Returns a [`DbError`] when the id is unknown, the underlying
    /// secret lookup fails, or the new adapter cannot be connected. The
    /// previous adapter remains live in that case — the swap is atomic
    /// or it does not happen at all.
    fn switch(&self, id: &str) -> Result<(), DbError>;
}

/// Bridge from a `Command::SwitchAiProvider { id }` to the actual swap
/// (ADR-0025). Symmetric with [`ConnectionSwitcher`] but ai-specific:
/// the swap targets the active `AiProvider` slot the binary owns rather
/// than the DB adapter slot. Failures surface as `AiError` because the
/// AI taxonomy is independent of `DbError` (ADR-0023 Decision 8).
///
/// The trait stays narrow on purpose. Given an `id` from
/// `ai-providers.toml`, the implementation:
///
/// * resolves the entry (returning `AiError::Configuration` when the id
///   is unknown);
/// * fetches the api key from the [`SecretStore`](dbboard_config::SecretStore)
///   (returning `AiError::Configuration` on miss);
/// * constructs the concrete provider (e.g. `AnthropicProvider`);
/// * swaps the live slot atomically.
///
/// The previous provider (if any) stays live when any step fails.
pub trait AiProviderSwitcher: Send + Sync + 'static {
    /// Swap the live AI provider to the entry named `id` from
    /// `ai-providers.toml`.
    ///
    /// # Errors
    ///
    /// Returns an [`AiError`] when the id is unknown, the api key
    /// lookup fails, or the new provider cannot be constructed. Like
    /// [`ConnectionSwitcher::switch`] the swap is atomic — on error the
    /// previous provider remains live.
    fn switch(&self, id: &str) -> Result<(), AiError>;
}

/// Spawn the worker thread. `base_url` is the loopback server root the
/// binary just started (e.g. `http://127.0.0.1:54123`). `switcher` is
/// the in-process bridge used to handle `SwitchConnection` commands.
/// `ai_provider_slot` is the shared, atomically-swappable AI provider
/// slot (ADR-0025): the worker reads a fresh snapshot off it on every
/// AI command dispatch, so a `SwitchAiProvider` performed by
/// `ai_switcher` becomes visible on the very next AI command without
/// any further plumbing. An empty slot (`None`) causes any Ai* command
/// to surface immediately as `Reply::AiFailed { AiError::Configuration }`
/// — defence-in-depth, since the UI panel is already gated on
/// `has_ai_provider()`.
pub(crate) fn spawn_worker(
    base_url: String,
    cmd_rx: Receiver<Command>,
    reply_tx: Sender<Reply>,
    egui_ctx: egui::Context,
    switcher: Arc<dyn ConnectionSwitcher>,
    ai_switcher: Arc<dyn AiProviderSwitcher>,
    ai_provider_slot: AiProviderSlot,
) {
    thread::Builder::new()
        .name("dbboard-http-worker".into())
        .spawn(move || {
            run_worker(
                &base_url,
                &cmd_rx,
                &reply_tx,
                &egui_ctx,
                switcher.as_ref(),
                ai_switcher.as_ref(),
                &ai_provider_slot,
            );
        })
        .expect("spawn dbboard-http-worker thread");
}

fn run_worker(
    base_url: &str,
    cmd_rx: &Receiver<Command>,
    reply_tx: &Sender<Reply>,
    egui_ctx: &egui::Context,
    switcher: &dyn ConnectionSwitcher,
    ai_switcher: &dyn AiProviderSwitcher,
    ai_provider_slot: &AiProviderSlot,
) {
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            return report_fatal(
                reply_tx,
                egui_ctx,
                &DbError::Connection(e.to_string()),
                cmd_rx,
            )
        }
    };
    let http = match reqwest::Client::builder().build() {
        Ok(client) => client,
        Err(e) => {
            return report_fatal(
                reply_tx,
                egui_ctx,
                &DbError::Connection(e.to_string()),
                cmd_rx,
            )
        }
    };

    while let Ok(cmd) = cmd_rx.recv() {
        // Snapshot the slot per dispatch so a swap performed between
        // commands becomes visible on the very next tick. The clone is
        // a single `Arc::clone` — the underlying provider is shared, not
        // copied. A poisoned lock is recovered into the inner value
        // because a writer panicking mid-swap leaves the slot in a
        // valid (Some or None) state.
        let snapshot: Option<Arc<dyn AiProvider>> = ai_provider_slot
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let reply = rt.block_on(dispatch(
            cmd,
            &http,
            base_url,
            switcher,
            ai_switcher,
            snapshot.as_deref(),
        ));
        if reply_tx.send(reply).is_err() {
            break; // UI side hung up — nothing left to answer.
        }
        egui_ctx.request_repaint();
    }
}

/// Pure dispatch: turn a single [`Command`] into the matching [`Reply`].
/// Extracted so the AI + switcher arms can be exercised under
/// `#[tokio::test]` without spawning a real worker thread or HTTP
/// server. The HTTP arms still need a reachable `base_url` so they are
/// tested indirectly through `client.rs`.
pub(crate) async fn dispatch(
    cmd: Command,
    http: &reqwest::Client,
    base_url: &str,
    switcher: &dyn ConnectionSwitcher,
    ai_switcher: &dyn AiProviderSwitcher,
    ai_provider: Option<&dyn AiProvider>,
) -> Reply {
    match cmd {
        // ADR-0020: SwitchConnection is in-process — no HTTP.
        Command::SwitchConnection { id } => match switcher.switch(&id) {
            Ok(()) => Reply::ConnectionSwitched { id },
            Err(error) => Reply::SwitchFailed { id, error },
        },
        // ADR-0025: SwitchAiProvider is also in-process. The switcher
        // owns the live `AiProvider` slot the dispatch arms read from
        // via `ai_provider`; the next dispatch tick will see the new
        // slot value transparently.
        Command::SwitchAiProvider { id } => match ai_switcher.switch(&id) {
            Ok(()) => Reply::AiProviderSwitched { id },
            Err(error) => Reply::AiProviderSwitchFailed {
                reason: error.to_string(),
            },
        },
        // ADR-0023: AI commands route to the injected provider. With no
        // provider the panel is hidden, but defence-in-depth: surface a
        // configuration error so a stray command never deadlocks the
        // panel's busy flag.
        Command::AiExplain { sql, dialect } => match ai_provider {
            Some(provider) => {
                let req = ExplainRequest { sql, dialect };
                ai_reply(provider.explain(&req).await)
            }
            None => no_provider_failure(),
        },
        Command::AiSuggest {
            prompt,
            dialect,
            schema,
        } => match ai_provider {
            Some(provider) => {
                let req = SuggestRequest {
                    prompt,
                    dialect,
                    schema,
                };
                ai_reply(provider.suggest_sql(&req).await)
            }
            None => no_provider_failure(),
        },
        // HTTP arms — unchanged from before slice (b).
        Command::ListTables | Command::Query(_) => {
            let request = client::request_for(&cmd);
            execute(http, base_url, &request).await
        }
    }
}

fn ai_reply(result: dbboard_ai::AiResult<dbboard_ai::AiResponse>) -> Reply {
    match result {
        Ok(resp) => Reply::AiResponded {
            text: resp.text,
            tokens_in: resp.tokens_in,
            tokens_out: resp.tokens_out,
        },
        Err(error) => Reply::AiFailed { error },
    }
}

fn no_provider_failure() -> Reply {
    Reply::AiFailed {
        error: AiError::Configuration(
            "no AI provider configured; set DBBOARD_ANTHROPIC_API_KEY".into(),
        ),
    }
}

async fn execute(http: &reqwest::Client, base_url: &str, request: &HttpRequest) -> Reply {
    match request {
        HttpRequest::GetTables => {
            let response = http.get(format!("{base_url}/tables")).send().await;
            match read(response).await {
                Ok((status, body)) => client::reply_for_tables(status, &body),
                Err(e) => Reply::Tables(Err(e)),
            }
        }
        HttpRequest::PostQuery(sql) => {
            let response = http
                .post(format!("{base_url}/query"))
                .json(&serde_json::json!({ "sql": sql }))
                .send()
                .await;
            match read(response).await {
                Ok((status, body)) => client::reply_for_query(status, &body),
                Err(e) => Reply::QueryResult(Err(e)),
            }
        }
    }
}

/// Collapse a `reqwest` send result into `(status, body)`, turning any
/// transport-level failure into a `Connection` error.
async fn read(
    response: Result<reqwest::Response, reqwest::Error>,
) -> Result<(u16, String), DbError> {
    let response = response.map_err(transport_error)?;
    let status = response.status().as_u16();
    let body = response.text().await.map_err(transport_error)?;
    Ok((status, body))
}

fn transport_error(err: reqwest::Error) -> DbError {
    // `without_url` strips the request URL from the message; it carries
    // no secrets here, but keeping errors URL-free is the safe default.
    DbError::Connection(format!("request failed: {}", err.without_url()))
}

/// The worker could not start its runtime or HTTP client. Echo the error
/// back and keep answering every command with it, so the UI surfaces the
/// failure instead of waiting forever for replies that will never come.
fn report_fatal(
    reply_tx: &Sender<Reply>,
    egui_ctx: &egui::Context,
    err: &DbError,
    cmd_rx: &Receiver<Command>,
) {
    let _ = reply_tx.send(Reply::Tables(Err(err.clone())));
    egui_ctx.request_repaint();

    while let Ok(cmd) = cmd_rx.recv() {
        let reply = match cmd {
            Command::ListTables => Reply::Tables(Err(err.clone())),
            Command::Query(_) => Reply::QueryResult(Err(err.clone())),
            // The worker never came up, so the in-process switcher
            // hand-off is unreachable too. Echo the same fatal error
            // back as a `SwitchFailed` so the UI can surface it.
            Command::SwitchConnection { id } => Reply::SwitchFailed {
                id,
                error: err.clone(),
            },
            // Same fate for AI commands: the worker has no provider
            // handle, so surface a configuration-style AI failure so
            // the panel exits busy state rather than waiting forever.
            // The error message preserves the underlying transport
            // failure (DbError::Connection) verbatim so the user sees
            // the actual cause.
            Command::AiExplain { .. } | Command::AiSuggest { .. } => Reply::AiFailed {
                error: AiError::Configuration(format!("ai worker unavailable: {}", err.message())),
            },
            // Same shape for the AI swap: the worker never built its
            // runtime, so any pending `SwitchAiProvider` fails fast
            // with the underlying transport error preserved.
            Command::SwitchAiProvider { .. } => Reply::AiProviderSwitchFailed {
                reason: format!("ai worker unavailable: {}", err.message()),
            },
        };
        if reply_tx.send(reply).is_err() {
            break;
        }
        egui_ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::{dispatch, AiProviderSwitcher, ConnectionSwitcher};
    use crate::{Command, Reply};
    use dbboard_ai::{
        AiCapabilities, AiError, AiProvider, AiResponse, AiResult, ExplainRequest, SuggestRequest,
    };
    use dbboard_core::DbError;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    /// Switcher stub that never gets called — AI tests do not exercise
    /// the switch path, but the dispatch fn requires a `&dyn` argument
    /// so we hand it a no-op.
    struct UnusedSwitcher;
    impl ConnectionSwitcher for UnusedSwitcher {
        fn switch(&self, _id: &str) -> Result<(), DbError> {
            unreachable!("dispatch test must not exercise SwitchConnection here")
        }
    }

    /// Mirror of [`UnusedSwitcher`] for the AI swap path — most
    /// dispatch tests do not exercise it but the fn signature requires
    /// a `&dyn AiProviderSwitcher`.
    struct UnusedAiSwitcher;
    impl AiProviderSwitcher for UnusedAiSwitcher {
        fn switch(&self, _id: &str) -> Result<(), AiError> {
            unreachable!("dispatch test must not exercise SwitchAiProvider here")
        }
    }

    /// Capturing stub: records the last id `switch` was called with and
    /// returns the configured outcome. Used by the `SwitchAiProvider`
    /// dispatch tests.
    struct StubAiSwitcher {
        outcome: AiSwitchOutcome,
        calls: AtomicUsize,
    }
    enum AiSwitchOutcome {
        Ok,
        Err(AiError),
    }
    impl AiProviderSwitcher for StubAiSwitcher {
        fn switch(&self, _id: &str) -> Result<(), AiError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match &self.outcome {
                AiSwitchOutcome::Ok => Ok(()),
                AiSwitchOutcome::Err(e) => Err(match e {
                    AiError::Configuration(s) => AiError::Configuration(s.clone()),
                    AiError::Network(s) => AiError::Network(s.clone()),
                    AiError::Provider(s) => AiError::Provider(s.clone()),
                    AiError::Quota(s) => AiError::Quota(s.clone()),
                    AiError::Cancelled => AiError::Cancelled,
                }),
            }
        }
    }

    /// Configurable AI provider stub. Each round-trip returns the same
    /// pre-staged outcome.
    struct StubProvider {
        kind: StubOutcome,
    }
    enum StubOutcome {
        Ok {
            text: String,
            tokens_in: u32,
            tokens_out: u32,
        },
        Err(AiError),
    }

    #[async_trait::async_trait]
    impl AiProvider for StubProvider {
        fn id(&self) -> &'static str {
            "stub"
        }
        fn capabilities(&self) -> AiCapabilities {
            AiCapabilities::default()
        }
        async fn explain(&self, _req: &ExplainRequest) -> AiResult<AiResponse> {
            self.outcome()
        }
        async fn suggest_sql(&self, _req: &SuggestRequest) -> AiResult<AiResponse> {
            self.outcome()
        }
    }
    impl StubProvider {
        fn outcome(&self) -> AiResult<AiResponse> {
            match &self.kind {
                StubOutcome::Ok {
                    text,
                    tokens_in,
                    tokens_out,
                } => Ok(AiResponse {
                    text: text.clone(),
                    tokens_in: *tokens_in,
                    tokens_out: *tokens_out,
                }),
                // AiError does not derive Clone (its variants are
                // one-shot), so we reconstruct the error here.
                StubOutcome::Err(e) => Err(match e {
                    AiError::Configuration(s) => AiError::Configuration(s.clone()),
                    AiError::Network(s) => AiError::Network(s.clone()),
                    AiError::Provider(s) => AiError::Provider(s.clone()),
                    AiError::Quota(s) => AiError::Quota(s.clone()),
                    AiError::Cancelled => AiError::Cancelled,
                }),
            }
        }
    }

    fn http_client() -> reqwest::Client {
        // The AI dispatch arms do not touch http; an empty client is
        // fine. `build()` is infallible for the default builder on
        // every supported platform.
        reqwest::Client::builder()
            .build()
            .expect("default reqwest client builds")
    }

    #[tokio::test]
    async fn dispatch_ai_explain_with_provider_returns_responded() {
        let provider: Arc<dyn AiProvider> = Arc::new(StubProvider {
            kind: StubOutcome::Ok {
                text: "this query selects one row".into(),
                tokens_in: 12,
                tokens_out: 34,
            },
        });
        let switcher = UnusedSwitcher;
        let ai_switcher = UnusedAiSwitcher;
        let http = http_client();
        let reply = dispatch(
            Command::AiExplain {
                sql: "SELECT 1".into(),
                dialect: Some("postgres".into()),
            },
            &http,
            "http://127.0.0.1:1",
            &switcher,
            &ai_switcher,
            Some(provider.as_ref()),
        )
        .await;
        match reply {
            Reply::AiResponded {
                text,
                tokens_in,
                tokens_out,
            } => {
                assert_eq!(text, "this query selects one row");
                assert_eq!(tokens_in, 12);
                assert_eq!(tokens_out, 34);
            }
            other => panic!("expected AiResponded, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn dispatch_ai_suggest_with_provider_returns_responded() {
        let provider: Arc<dyn AiProvider> = Arc::new(StubProvider {
            kind: StubOutcome::Ok {
                text: "SELECT 1".into(),
                tokens_in: 1,
                tokens_out: 2,
            },
        });
        let switcher = UnusedSwitcher;
        let ai_switcher = UnusedAiSwitcher;
        let http = http_client();
        let reply = dispatch(
            Command::AiSuggest {
                prompt: "monthly active users".into(),
                dialect: None,
                schema: Vec::new(),
            },
            &http,
            "http://127.0.0.1:1",
            &switcher,
            &ai_switcher,
            Some(provider.as_ref()),
        )
        .await;
        assert!(matches!(
            reply,
            Reply::AiResponded { text, .. } if text == "SELECT 1"
        ));
    }

    #[tokio::test]
    async fn dispatch_ai_explain_with_provider_error_returns_failed() {
        let provider: Arc<dyn AiProvider> = Arc::new(StubProvider {
            kind: StubOutcome::Err(AiError::Provider("rate_limit".into())),
        });
        let switcher = UnusedSwitcher;
        let ai_switcher = UnusedAiSwitcher;
        let http = http_client();
        let reply = dispatch(
            Command::AiExplain {
                sql: "SELECT 1".into(),
                dialect: None,
            },
            &http,
            "http://127.0.0.1:1",
            &switcher,
            &ai_switcher,
            Some(provider.as_ref()),
        )
        .await;
        match reply {
            Reply::AiFailed { error } => {
                assert!(matches!(error, AiError::Provider(msg) if msg == "rate_limit"));
            }
            other => panic!("expected AiFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn dispatch_ai_command_without_provider_yields_configuration_failure() {
        let switcher = UnusedSwitcher;
        let ai_switcher = UnusedAiSwitcher;
        let http = http_client();
        let reply = dispatch(
            Command::AiExplain {
                sql: "SELECT 1".into(),
                dialect: None,
            },
            &http,
            "http://127.0.0.1:1",
            &switcher,
            &ai_switcher,
            None,
        )
        .await;
        match reply {
            Reply::AiFailed {
                error: AiError::Configuration(msg),
            } => {
                assert!(
                    msg.contains("DBBOARD_ANTHROPIC_API_KEY") || msg.contains("provider"),
                    "configuration error should mention the env var or provider gate: {msg}"
                );
            }
            other => panic!("expected AiFailed(Configuration), got {other:?}"),
        }
    }

    #[tokio::test]
    async fn dispatch_switch_connection_short_circuits_before_http() {
        // Smoke test the existing SwitchConnection arm still routes
        // through the dispatch fn after refactoring.
        struct OkSwitcher;
        impl ConnectionSwitcher for OkSwitcher {
            fn switch(&self, _id: &str) -> Result<(), DbError> {
                Ok(())
            }
        }
        let switcher = OkSwitcher;
        let ai_switcher = UnusedAiSwitcher;
        let http = http_client();
        let reply = dispatch(
            Command::SwitchConnection {
                id: "prod-pg".into(),
            },
            &http,
            "http://127.0.0.1:1",
            &switcher,
            &ai_switcher,
            None,
        )
        .await;
        assert!(matches!(reply, Reply::ConnectionSwitched { id } if id == "prod-pg"));
    }

    // --- ADR-0025: SwitchAiProvider dispatch tests ----------------------

    #[tokio::test]
    async fn dispatch_switch_ai_provider_returns_switched_on_success() {
        let switcher = UnusedSwitcher;
        let ai_switcher = StubAiSwitcher {
            outcome: AiSwitchOutcome::Ok,
            calls: AtomicUsize::new(0),
        };
        let http = http_client();
        let reply = dispatch(
            Command::SwitchAiProvider {
                id: "anthropic-prod".into(),
            },
            &http,
            "http://127.0.0.1:1",
            &switcher,
            &ai_switcher,
            None,
        )
        .await;
        assert_eq!(
            ai_switcher.calls.load(Ordering::SeqCst),
            1,
            "switcher must be called exactly once"
        );
        assert!(matches!(reply, Reply::AiProviderSwitched { id } if id == "anthropic-prod"));
    }

    #[tokio::test]
    async fn dispatch_switch_ai_provider_returns_switch_failed_on_error() {
        let switcher = UnusedSwitcher;
        let ai_switcher = StubAiSwitcher {
            outcome: AiSwitchOutcome::Err(AiError::Configuration("unknown id".into())),
            calls: AtomicUsize::new(0),
        };
        let http = http_client();
        let reply = dispatch(
            Command::SwitchAiProvider {
                id: "missing".into(),
            },
            &http,
            "http://127.0.0.1:1",
            &switcher,
            &ai_switcher,
            None,
        )
        .await;
        match reply {
            // Reason is the AiError::Display text — not the raw enum
            // variant — because the AI taxonomy is not exposed on this
            // reply (ADR-0023 Decision 8 keeps AiError out of the
            // cross-channel UI message types).
            Reply::AiProviderSwitchFailed { reason } => {
                assert!(
                    reason.contains("unknown id"),
                    "reason should carry the AiError display text: {reason}"
                );
            }
            other => panic!("expected AiProviderSwitchFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn dispatch_switch_ai_provider_does_not_touch_ai_provider_slot() {
        // The swap is owned by the switcher, not by the read-side
        // `ai_provider` arg the dispatch fn carries for AI* commands.
        // Pass `None` for the read slot to prove dispatch never reads
        // it during a swap.
        let switcher = UnusedSwitcher;
        let ai_switcher = StubAiSwitcher {
            outcome: AiSwitchOutcome::Ok,
            calls: AtomicUsize::new(0),
        };
        let http = http_client();
        let reply = dispatch(
            Command::SwitchAiProvider { id: "x".into() },
            &http,
            "http://127.0.0.1:1",
            &switcher,
            &ai_switcher,
            None,
        )
        .await;
        assert!(matches!(reply, Reply::AiProviderSwitched { .. }));
    }
}
