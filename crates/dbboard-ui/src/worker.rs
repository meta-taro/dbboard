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
//!
//! ADR-0026 (Group B, AI streaming + cancel) keeps the `std::mpsc` <-> UI
//! contract unchanged but rewires the inside of the worker. The main
//! loop now runs on the runtime (`rt.block_on(async { ... })`) so the
//! AI streaming arms can `tokio::spawn` a per-request task and the main
//! loop can keep draining commands while a stream is in flight. A
//! short bridge thread shuttles `std::mpsc` commands into a
//! `tokio::mpsc` channel so the main loop can `.await` on `recv()`. A
//! single-slot `Option<CancellationToken>` tracks the in-flight AI
//! request; `Command::CancelAiRequest` `.cancel()`s it, the spawned
//! task observes the cancellation via `tokio::select!` and emits
//! `Reply::AiCancelled`.

use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, PoisonError, RwLock};
use std::thread;

use dbboard_ai::{
    AiError, AiProvider, AiStream, ExplainRequest, StopReason, StreamEvent, SuggestRequest,
};
use dbboard_core::DbError;
use eframe::egui;
use futures_util::StreamExt;
use tokio::sync::mpsc as tmpsc;
use tokio_util::sync::CancellationToken;

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
                cmd_rx,
                reply_tx,
                egui_ctx,
                switcher,
                ai_switcher,
                ai_provider_slot,
            );
        })
        .expect("spawn dbboard-http-worker thread");
}

fn run_worker(
    base_url: &str,
    cmd_rx: Receiver<Command>,
    reply_tx: Sender<Reply>,
    egui_ctx: egui::Context,
    switcher: Arc<dyn ConnectionSwitcher>,
    ai_switcher: Arc<dyn AiProviderSwitcher>,
    ai_provider_slot: AiProviderSlot,
) {
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            return report_fatal(
                &reply_tx,
                &egui_ctx,
                &DbError::Connection(e.to_string()),
                &cmd_rx,
            )
        }
    };
    let http = match reqwest::Client::builder().build() {
        Ok(client) => client,
        Err(e) => {
            return report_fatal(
                &reply_tx,
                &egui_ctx,
                &DbError::Connection(e.to_string()),
                &cmd_rx,
            )
        }
    };

    // ADR-0026: bridge the synchronous UI-side `cmd_rx` onto a
    // tokio mpsc channel so the main loop can `.await recv()` and
    // share the runtime with spawned streaming tasks. The bridge
    // thread exits when the UI side hangs up; the tokio receiver
    // then yields `None` and the runtime block returns.
    let (tokio_cmd_tx, tokio_cmd_rx) = tmpsc::unbounded_channel::<Command>();
    thread::Builder::new()
        .name("dbboard-cmd-bridge".into())
        .spawn(move || {
            while let Ok(cmd) = cmd_rx.recv() {
                if tokio_cmd_tx.send(cmd).is_err() {
                    break;
                }
            }
        })
        .expect("spawn dbboard-cmd-bridge thread");

    rt.block_on(run_command_loop(
        base_url,
        tokio_cmd_rx,
        reply_tx,
        egui_ctx,
        http,
        switcher,
        ai_switcher,
        ai_provider_slot,
    ));
}

#[allow(clippy::too_many_arguments)]
async fn run_command_loop(
    base_url: &str,
    mut cmd_rx: tmpsc::UnboundedReceiver<Command>,
    reply_tx: Sender<Reply>,
    egui_ctx: egui::Context,
    http: reqwest::Client,
    switcher: Arc<dyn ConnectionSwitcher>,
    ai_switcher: Arc<dyn AiProviderSwitcher>,
    ai_provider_slot: AiProviderSlot,
) {
    // Single-slot in-flight AI cancel handle. ADR-0026's UI gates every
    // AI command on `busy`, so at most one AI request is in flight at
    // any time — a HashMap is unnecessary. A stale Some value left
    // after a task completes naturally is harmless: the next AI command
    // overwrites it, and a CancelAiRequest on it just `.cancel()`s a
    // token nobody is listening on.
    let mut in_flight: Option<CancellationToken> = None;
    while let Some(cmd) = cmd_rx.recv().await {
        handle_command(
            cmd,
            &mut in_flight,
            base_url,
            &reply_tx,
            &egui_ctx,
            &http,
            switcher.as_ref(),
            ai_switcher.as_ref(),
            &ai_provider_slot,
        )
        .await;
    }
}

// Length is dominated by per-variant match arms (each ~6–10 lines of
// payload destructuring + spawn_ai_task call). Splitting further would
// just push the variant payloads into single-use helpers without
// improving readability — the dispatch *is* the matrix.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn handle_command(
    cmd: Command,
    in_flight: &mut Option<CancellationToken>,
    base_url: &str,
    reply_tx: &Sender<Reply>,
    egui_ctx: &egui::Context,
    http: &reqwest::Client,
    switcher: &dyn ConnectionSwitcher,
    ai_switcher: &dyn AiProviderSwitcher,
    ai_provider_slot: &AiProviderSlot,
) {
    match cmd {
        // ADR-0026 Decision 5/10/12: cancel the in-flight AI request,
        // if any. The spawned task emits AiCancelled via the `select!`
        // cancel arm on its way out.
        Command::CancelAiRequest => {
            if let Some(token) = in_flight.take() {
                token.cancel();
            }
        }
        // ADR-0026 streaming arms: spawn per-request and continue the
        // main loop so subsequent commands (including CancelAiRequest)
        // are still drained.
        Command::AiExplainStream { sql, dialect } => {
            spawn_ai_task(
                in_flight,
                ai_provider_slot,
                reply_tx,
                egui_ctx,
                |p, t, tx, c| {
                    Box::pin(run_explain_stream(
                        p,
                        ExplainRequest { sql, dialect },
                        t,
                        tx,
                        c,
                    ))
                },
            );
        }
        Command::AiSuggestStream {
            prompt,
            dialect,
            schema,
        } => {
            spawn_ai_task(
                in_flight,
                ai_provider_slot,
                reply_tx,
                egui_ctx,
                |p, t, tx, c| {
                    Box::pin(run_suggest_stream(
                        p,
                        SuggestRequest {
                            prompt,
                            dialect,
                            schema,
                        },
                        t,
                        tx,
                        c,
                    ))
                },
            );
        }
        // ADR-0026 Decision 10: the atomic path also routes through the
        // cancel race so a Cancel arriving mid-`explain` resets the
        // panel rather than billing for completion.
        Command::AiExplain { sql, dialect } => {
            spawn_ai_task(
                in_flight,
                ai_provider_slot,
                reply_tx,
                egui_ctx,
                |p, t, tx, c| {
                    Box::pin(run_explain_atomic(
                        p,
                        ExplainRequest { sql, dialect },
                        t,
                        tx,
                        c,
                    ))
                },
            );
        }
        Command::AiSuggest {
            prompt,
            dialect,
            schema,
        } => {
            spawn_ai_task(
                in_flight,
                ai_provider_slot,
                reply_tx,
                egui_ctx,
                |p, t, tx, c| {
                    Box::pin(run_suggest_atomic(
                        p,
                        SuggestRequest {
                            prompt,
                            dialect,
                            schema,
                        },
                        t,
                        tx,
                        c,
                    ))
                },
            );
        }
        // Provider/connection swaps stay inline — they are fast
        // in-process operations that complete before the next command
        // would benefit from concurrency.
        Command::SwitchConnection { id } => {
            let reply = match switcher.switch(&id) {
                Ok(()) => Reply::ConnectionSwitched { id },
                Err(error) => Reply::SwitchFailed { id, error },
            };
            let _ = reply_tx.send(reply);
            egui_ctx.request_repaint();
        }
        Command::SwitchAiProvider { id } => {
            let reply = match ai_switcher.switch(&id) {
                Ok(()) => Reply::AiProviderSwitched { id },
                Err(error) => Reply::AiProviderSwitchFailed {
                    reason: error.to_string(),
                },
            };
            let _ = reply_tx.send(reply);
            egui_ctx.request_repaint();
        }
        // HTTP arms — short round-trips, awaited inline.
        cmd @ (Command::ListTables | Command::Query(_)) => {
            let request = client::request_for(&cmd);
            let reply = execute(http, base_url, &request).await;
            let _ = reply_tx.send(reply);
            egui_ctx.request_repaint();
        }
    }
}

/// Wire up an AI task: snapshot the provider, install a fresh cancel
/// token in `in_flight`, and `tokio::spawn` the runner the caller
/// builds. The runner is built lazily via the closure so each variant
/// can own its request payload.
fn spawn_ai_task<F>(
    in_flight: &mut Option<CancellationToken>,
    ai_provider_slot: &AiProviderSlot,
    reply_tx: &Sender<Reply>,
    egui_ctx: &egui::Context,
    runner: F,
) where
    F: FnOnce(
        Option<Arc<dyn AiProvider>>,
        CancellationToken,
        Sender<Reply>,
        egui::Context,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>,
{
    let token = CancellationToken::new();
    *in_flight = Some(token.clone());
    let provider = snapshot_provider(ai_provider_slot);
    let reply_tx = reply_tx.clone();
    let ctx = egui_ctx.clone();
    let fut = runner(provider, token, reply_tx, ctx);
    tokio::spawn(fut);
}

fn snapshot_provider(slot: &AiProviderSlot) -> Option<Arc<dyn AiProvider>> {
    // Snapshot per dispatch so a swap between commands becomes visible
    // on the next one. A poisoned lock is recovered into the inner
    // value because a writer panicking mid-swap leaves the slot in a
    // valid (Some or None) state.
    slot.read().unwrap_or_else(PoisonError::into_inner).clone()
}

/// ADR-0026 Decision 10: atomic explain through the cancel race.
pub(crate) async fn run_explain_atomic(
    provider: Option<Arc<dyn AiProvider>>,
    req: ExplainRequest,
    token: CancellationToken,
    reply_tx: Sender<Reply>,
    egui_ctx: egui::Context,
) {
    let Some(provider) = provider else {
        let _ = reply_tx.send(no_provider_failure());
        egui_ctx.request_repaint();
        return;
    };
    let reply = tokio::select! {
        biased;
        () = token.cancelled() => Reply::AiCancelled,
        result = provider.explain(&req) => ai_reply(result),
    };
    let _ = reply_tx.send(reply);
    egui_ctx.request_repaint();
}

/// ADR-0026 Decision 10: atomic `suggest_sql` through the cancel race.
pub(crate) async fn run_suggest_atomic(
    provider: Option<Arc<dyn AiProvider>>,
    req: SuggestRequest,
    token: CancellationToken,
    reply_tx: Sender<Reply>,
    egui_ctx: egui::Context,
) {
    let Some(provider) = provider else {
        let _ = reply_tx.send(no_provider_failure());
        egui_ctx.request_repaint();
        return;
    };
    let reply = tokio::select! {
        biased;
        () = token.cancelled() => Reply::AiCancelled,
        result = provider.suggest_sql(&req) => ai_reply(result),
    };
    let _ = reply_tx.send(reply);
    egui_ctx.request_repaint();
}

/// ADR-0026 Decisions 5/6/12: open a streaming explain, race the
/// per-chunk `next()` against the cancel token, and forward each
/// chunk over the reply channel.
pub(crate) async fn run_explain_stream(
    provider: Option<Arc<dyn AiProvider>>,
    req: ExplainRequest,
    token: CancellationToken,
    reply_tx: Sender<Reply>,
    egui_ctx: egui::Context,
) {
    let Some(provider) = provider else {
        let _ = reply_tx.send(no_provider_failure());
        egui_ctx.request_repaint();
        return;
    };
    let open = tokio::select! {
        biased;
        () = token.cancelled() => {
            let _ = reply_tx.send(Reply::AiCancelled);
            egui_ctx.request_repaint();
            return;
        }
        result = provider.stream_explain(&req) => result,
    };
    forward_stream(open, token, reply_tx, egui_ctx).await;
}

/// ADR-0026 Decisions 5/6/12: streaming `suggest_sql`, same shape as
/// [`run_explain_stream`].
pub(crate) async fn run_suggest_stream(
    provider: Option<Arc<dyn AiProvider>>,
    req: SuggestRequest,
    token: CancellationToken,
    reply_tx: Sender<Reply>,
    egui_ctx: egui::Context,
) {
    let Some(provider) = provider else {
        let _ = reply_tx.send(no_provider_failure());
        egui_ctx.request_repaint();
        return;
    };
    let open = tokio::select! {
        biased;
        () = token.cancelled() => {
            let _ = reply_tx.send(Reply::AiCancelled);
            egui_ctx.request_repaint();
            return;
        }
        result = provider.stream_suggest_sql(&req) => result,
    };
    forward_stream(open, token, reply_tx, egui_ctx).await;
}

/// Drive an opened [`AiStream`] to completion, forwarding each event
/// onto the reply channel and racing every `next()` against the
/// cancel token. Cumulative token counts are tracked locally so the
/// terminal `AiStreamComplete` carries the last-known values even when
/// the final `message_delta` does not repeat them.
async fn forward_stream(
    opened: dbboard_ai::AiResult<AiStream>,
    token: CancellationToken,
    reply_tx: Sender<Reply>,
    egui_ctx: egui::Context,
) {
    let mut stream = match opened {
        Ok(s) => s,
        Err(error) => {
            let _ = reply_tx.send(Reply::AiFailed { error });
            egui_ctx.request_repaint();
            return;
        }
    };
    let mut last_tokens_in: u32 = 0;
    let mut last_tokens_out: u32 = 0;
    loop {
        let next = tokio::select! {
            biased;
            () = token.cancelled() => {
                drop(stream);
                let _ = reply_tx.send(Reply::AiCancelled);
                egui_ctx.request_repaint();
                return;
            }
            next = stream.next() => next,
        };
        match next {
            None => {
                // Stream exhausted without MessageStop. The
                // dbboard-anthropic adapter ships a defensive
                // MessageStop on early close (ADR-0026 Decision 6),
                // but a non-Anthropic provider using the default
                // delegate might still end here; emit a synthetic
                // EndTurn so the panel always sees a terminator.
                let _ = reply_tx.send(Reply::AiStreamComplete {
                    tokens_in: last_tokens_in,
                    tokens_out: last_tokens_out,
                    stop_reason: StopReason::EndTurn,
                });
                egui_ctx.request_repaint();
                return;
            }
            Some(Err(error)) => {
                let _ = reply_tx.send(Reply::AiFailed { error });
                egui_ctx.request_repaint();
                return;
            }
            Some(Ok(event)) => match event {
                StreamEvent::MessageStart { tokens_in } => {
                    last_tokens_in = tokens_in;
                    let _ = reply_tx.send(Reply::AiChunk {
                        text_delta: String::new(),
                        tokens_in: Some(tokens_in),
                        tokens_out: None,
                    });
                    egui_ctx.request_repaint();
                }
                StreamEvent::TextDelta(text) => {
                    let _ = reply_tx.send(Reply::AiChunk {
                        text_delta: text,
                        tokens_in: None,
                        tokens_out: None,
                    });
                    egui_ctx.request_repaint();
                }
                StreamEvent::Usage {
                    tokens_in,
                    tokens_out,
                } => {
                    last_tokens_in = tokens_in;
                    last_tokens_out = tokens_out;
                    let _ = reply_tx.send(Reply::AiChunk {
                        text_delta: String::new(),
                        tokens_in: Some(tokens_in),
                        tokens_out: Some(tokens_out),
                    });
                    egui_ctx.request_repaint();
                }
                StreamEvent::MessageStop { stop_reason } => {
                    let _ = reply_tx.send(Reply::AiStreamComplete {
                        tokens_in: last_tokens_in,
                        tokens_out: last_tokens_out,
                        stop_reason,
                    });
                    egui_ctx.request_repaint();
                    return;
                }
                StreamEvent::Error(error) => {
                    let _ = reply_tx.send(Reply::AiFailed { error });
                    egui_ctx.request_repaint();
                    return;
                }
            },
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
            Command::SwitchConnection { id } => Reply::SwitchFailed {
                id,
                error: err.clone(),
            },
            Command::AiExplain { .. }
            | Command::AiSuggest { .. }
            | Command::AiExplainStream { .. }
            | Command::AiSuggestStream { .. } => Reply::AiFailed {
                error: AiError::Configuration(format!("ai worker unavailable: {}", err.message())),
            },
            // ADR-0026 Decision 5: a cancel arriving on the fatal path
            // is acknowledged with AiCancelled so the panel exits busy
            // — the request the user wanted to cancel was never
            // dispatched in the first place.
            Command::CancelAiRequest => Reply::AiCancelled,
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
    use super::{
        run_explain_atomic, run_explain_stream, run_suggest_atomic, run_suggest_stream,
        AiProviderSwitcher, ConnectionSwitcher,
    };
    use crate::Reply;
    use dbboard_ai::{
        AiCapabilities, AiError, AiProvider, AiResponse, AiResult, AiStream, ExplainRequest,
        StopReason, StreamEvent, SuggestRequest,
    };
    use dbboard_core::DbError;
    use eframe::egui;
    use futures_util::stream;
    use std::sync::mpsc;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;

    /// Switcher stub that never gets called.
    #[allow(dead_code)]
    struct UnusedSwitcher;
    impl ConnectionSwitcher for UnusedSwitcher {
        fn switch(&self, _id: &str) -> Result<(), DbError> {
            unreachable!("dispatch test must not exercise SwitchConnection here")
        }
    }

    #[allow(dead_code)]
    struct UnusedAiSwitcher;
    impl AiProviderSwitcher for UnusedAiSwitcher {
        fn switch(&self, _id: &str) -> Result<(), AiError> {
            unreachable!("dispatch test must not exercise SwitchAiProvider here")
        }
    }

    /// Stub provider: streams the pre-staged events on every call.
    struct StubProvider {
        events: Vec<AiResult<StreamEvent>>,
        explain_response: AiResult<AiResponse>,
        suggest_response: AiResult<AiResponse>,
        calls: AtomicUsize,
    }

    impl StubProvider {
        fn streaming(events: Vec<AiResult<StreamEvent>>) -> Self {
            Self {
                events,
                explain_response: Ok(AiResponse {
                    text: "unused".into(),
                    tokens_in: 0,
                    tokens_out: 0,
                }),
                suggest_response: Ok(AiResponse {
                    text: "unused".into(),
                    tokens_in: 0,
                    tokens_out: 0,
                }),
                calls: AtomicUsize::new(0),
            }
        }

        fn atomic_ok(text: &str, tokens_in: u32, tokens_out: u32) -> Self {
            Self {
                events: Vec::new(),
                explain_response: Ok(AiResponse {
                    text: text.into(),
                    tokens_in,
                    tokens_out,
                }),
                suggest_response: Ok(AiResponse {
                    text: text.into(),
                    tokens_in,
                    tokens_out,
                }),
                calls: AtomicUsize::new(0),
            }
        }

        fn clone_events(&self) -> Vec<AiResult<StreamEvent>> {
            self.events
                .iter()
                .map(|r| match r {
                    Ok(StreamEvent::MessageStart { tokens_in }) => Ok(StreamEvent::MessageStart {
                        tokens_in: *tokens_in,
                    }),
                    Ok(StreamEvent::TextDelta(s)) => Ok(StreamEvent::TextDelta(s.clone())),
                    Ok(StreamEvent::Usage {
                        tokens_in,
                        tokens_out,
                    }) => Ok(StreamEvent::Usage {
                        tokens_in: *tokens_in,
                        tokens_out: *tokens_out,
                    }),
                    Ok(StreamEvent::MessageStop { stop_reason }) => Ok(StreamEvent::MessageStop {
                        stop_reason: stop_reason.clone(),
                    }),
                    Ok(StreamEvent::Error(e)) => Ok(StreamEvent::Error(reclone_error(e))),
                    Err(e) => Err(reclone_error(e)),
                })
                .collect()
        }

        fn clone_response(r: &AiResult<AiResponse>) -> AiResult<AiResponse> {
            match r {
                Ok(resp) => Ok(AiResponse {
                    text: resp.text.clone(),
                    tokens_in: resp.tokens_in,
                    tokens_out: resp.tokens_out,
                }),
                Err(e) => Err(reclone_error(e)),
            }
        }
    }

    fn reclone_error(e: &AiError) -> AiError {
        match e {
            AiError::Configuration(s) => AiError::Configuration(s.clone()),
            AiError::Network(s) => AiError::Network(s.clone()),
            AiError::Provider(s) => AiError::Provider(s.clone()),
            AiError::Quota(s) => AiError::Quota(s.clone()),
            AiError::Cancelled => AiError::Cancelled,
        }
    }

    #[async_trait::async_trait]
    impl AiProvider for StubProvider {
        fn id(&self) -> &'static str {
            "stub"
        }
        fn capabilities(&self) -> AiCapabilities {
            AiCapabilities {
                has_streaming: true,
                has_function_calling: false,
            }
        }
        async fn explain(&self, _req: &ExplainRequest) -> AiResult<AiResponse> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            StubProvider::clone_response(&self.explain_response)
        }
        async fn suggest_sql(&self, _req: &SuggestRequest) -> AiResult<AiResponse> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            StubProvider::clone_response(&self.suggest_response)
        }
        async fn stream_explain(&self, _req: &ExplainRequest) -> AiResult<AiStream> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(Box::pin(stream::iter(self.clone_events())))
        }
        async fn stream_suggest_sql(&self, _req: &SuggestRequest) -> AiResult<AiStream> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(Box::pin(stream::iter(self.clone_events())))
        }
    }

    fn ctx() -> egui::Context {
        egui::Context::default()
    }

    fn explain_req() -> ExplainRequest {
        ExplainRequest {
            sql: "SELECT 1".into(),
            dialect: Some("postgres".into()),
        }
    }

    fn suggest_req() -> SuggestRequest {
        SuggestRequest {
            prompt: "active users".into(),
            dialect: None,
            schema: Vec::new(),
        }
    }

    fn drain(rx: &mpsc::Receiver<Reply>) -> Vec<Reply> {
        let mut out = Vec::new();
        while let Ok(r) = rx.try_recv() {
            out.push(r);
        }
        out
    }

    // ---- streaming happy path -----------------------------------------

    #[tokio::test]
    async fn run_explain_stream_forwards_start_deltas_usage_and_complete_in_order() {
        let provider: Arc<dyn AiProvider> = Arc::new(StubProvider::streaming(vec![
            Ok(StreamEvent::MessageStart { tokens_in: 11 }),
            Ok(StreamEvent::TextDelta("Hello".into())),
            Ok(StreamEvent::TextDelta(" world".into())),
            Ok(StreamEvent::Usage {
                tokens_in: 11,
                tokens_out: 7,
            }),
            Ok(StreamEvent::MessageStop {
                stop_reason: StopReason::EndTurn,
            }),
        ]));
        let (tx, rx) = mpsc::channel::<Reply>();
        let token = CancellationToken::new();
        run_explain_stream(Some(provider), explain_req(), token, tx, ctx()).await;
        let replies = drain(&rx);
        assert_eq!(replies.len(), 5, "5 replies: 4 chunks + complete");
        // First chunk = MessageStart with tokens_in only
        match &replies[0] {
            Reply::AiChunk {
                text_delta,
                tokens_in,
                tokens_out,
            } => {
                assert!(text_delta.is_empty());
                assert_eq!(*tokens_in, Some(11));
                assert_eq!(*tokens_out, None);
            }
            other => panic!("expected AiChunk for MessageStart, got {other:?}"),
        }
        // Two text deltas
        assert!(matches!(&replies[1], Reply::AiChunk { text_delta, .. } if text_delta == "Hello"));
        assert!(matches!(&replies[2], Reply::AiChunk { text_delta, .. } if text_delta == " world"));
        // Usage carries both counts cumulatively
        match &replies[3] {
            Reply::AiChunk {
                text_delta,
                tokens_in,
                tokens_out,
            } => {
                assert!(text_delta.is_empty());
                assert_eq!(*tokens_in, Some(11));
                assert_eq!(*tokens_out, Some(7));
            }
            other => panic!("expected AiChunk for Usage, got {other:?}"),
        }
        // Terminal complete with the last-known cumulative counts
        match &replies[4] {
            Reply::AiStreamComplete {
                tokens_in,
                tokens_out,
                stop_reason,
            } => {
                assert_eq!(*tokens_in, 11);
                assert_eq!(*tokens_out, 7);
                assert!(matches!(stop_reason, StopReason::EndTurn));
            }
            other => panic!("expected AiStreamComplete, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn run_suggest_stream_forwards_chunks_then_complete() {
        // Smoke test: stream_suggest_sql goes through the same
        // forward_stream pipeline as stream_explain.
        let provider: Arc<dyn AiProvider> = Arc::new(StubProvider::streaming(vec![
            Ok(StreamEvent::TextDelta("SELECT 1".into())),
            Ok(StreamEvent::MessageStop {
                stop_reason: StopReason::EndTurn,
            }),
        ]));
        let (tx, rx) = mpsc::channel::<Reply>();
        run_suggest_stream(
            Some(provider),
            suggest_req(),
            CancellationToken::new(),
            tx,
            ctx(),
        )
        .await;
        let replies = drain(&rx);
        assert_eq!(replies.len(), 2);
        assert!(
            matches!(&replies[0], Reply::AiChunk { text_delta, .. } if text_delta == "SELECT 1")
        );
        assert!(matches!(&replies[1], Reply::AiStreamComplete { .. }));
    }

    // ---- streaming error paths ----------------------------------------

    #[tokio::test]
    async fn mid_stream_error_event_surfaces_as_ai_failed_and_terminates() {
        let provider: Arc<dyn AiProvider> = Arc::new(StubProvider::streaming(vec![
            Ok(StreamEvent::MessageStart { tokens_in: 5 }),
            Ok(StreamEvent::TextDelta("partial".into())),
            Ok(StreamEvent::Error(AiError::Provider("overloaded".into()))),
            // Anything after Error must not be forwarded.
            Ok(StreamEvent::TextDelta("never seen".into())),
        ]));
        let (tx, rx) = mpsc::channel::<Reply>();
        run_explain_stream(
            Some(provider),
            explain_req(),
            CancellationToken::new(),
            tx,
            ctx(),
        )
        .await;
        let replies = drain(&rx);
        assert_eq!(replies.len(), 3, "MessageStart + TextDelta + AiFailed");
        assert!(
            matches!(&replies[2], Reply::AiFailed { error } if matches!(error, AiError::Provider(s) if s == "overloaded"))
        );
    }

    #[tokio::test]
    async fn outer_stream_err_surfaces_as_ai_failed_and_terminates() {
        let provider: Arc<dyn AiProvider> = Arc::new(StubProvider::streaming(vec![
            Ok(StreamEvent::TextDelta("partial".into())),
            Err(AiError::Network("conn reset".into())),
            Ok(StreamEvent::TextDelta("never seen".into())),
        ]));
        let (tx, rx) = mpsc::channel::<Reply>();
        run_explain_stream(
            Some(provider),
            explain_req(),
            CancellationToken::new(),
            tx,
            ctx(),
        )
        .await;
        let replies = drain(&rx);
        assert_eq!(replies.len(), 2);
        assert!(
            matches!(&replies[1], Reply::AiFailed { error } if matches!(error, AiError::Network(s) if s == "conn reset"))
        );
    }

    #[tokio::test]
    async fn stream_without_terminator_emits_synthetic_complete() {
        let provider: Arc<dyn AiProvider> = Arc::new(StubProvider::streaming(vec![
            Ok(StreamEvent::TextDelta("partial".into())),
            // No MessageStop — exercise the synthetic terminator path.
        ]));
        let (tx, rx) = mpsc::channel::<Reply>();
        run_explain_stream(
            Some(provider),
            explain_req(),
            CancellationToken::new(),
            tx,
            ctx(),
        )
        .await;
        let replies = drain(&rx);
        assert_eq!(replies.len(), 2);
        assert!(matches!(
            &replies[1],
            Reply::AiStreamComplete {
                stop_reason: StopReason::EndTurn,
                ..
            }
        ));
    }

    // ---- cancel paths --------------------------------------------------

    #[tokio::test]
    async fn streaming_cancel_during_first_chunk_surfaces_ai_cancelled() {
        // A provider whose stream never produces a chunk — `next()`
        // pends forever. Cancelling the token fires the select! cancel
        // arm and emits AiCancelled.
        struct PendingStreamProvider;
        #[async_trait::async_trait]
        impl AiProvider for PendingStreamProvider {
            fn id(&self) -> &'static str {
                "pending"
            }
            fn capabilities(&self) -> AiCapabilities {
                AiCapabilities {
                    has_streaming: true,
                    has_function_calling: false,
                }
            }
            async fn explain(&self, _req: &ExplainRequest) -> AiResult<AiResponse> {
                unreachable!()
            }
            async fn suggest_sql(&self, _req: &SuggestRequest) -> AiResult<AiResponse> {
                unreachable!()
            }
            async fn stream_explain(&self, _req: &ExplainRequest) -> AiResult<AiStream> {
                Ok(Box::pin(stream::pending()))
            }
            async fn stream_suggest_sql(&self, _req: &SuggestRequest) -> AiResult<AiStream> {
                unreachable!()
            }
        }
        let provider: Arc<dyn AiProvider> = Arc::new(PendingStreamProvider);
        let (tx, rx) = mpsc::channel::<Reply>();
        let token = CancellationToken::new();
        let token_clone = token.clone();
        // Fire the cancel from a separate task so the main test task
        // is the one awaiting run_explain_stream.
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            token_clone.cancel();
        });
        run_explain_stream(Some(provider), explain_req(), token, tx, ctx()).await;
        let replies = drain(&rx);
        assert_eq!(replies.len(), 1);
        assert!(matches!(&replies[0], Reply::AiCancelled));
    }

    #[tokio::test]
    async fn atomic_cancel_before_completion_surfaces_ai_cancelled() {
        // Provider whose explain future pends forever. The cancel arm
        // of the select! fires and emits AiCancelled.
        struct PendingAtomicProvider;
        #[async_trait::async_trait]
        impl AiProvider for PendingAtomicProvider {
            fn id(&self) -> &'static str {
                "pending-atomic"
            }
            fn capabilities(&self) -> AiCapabilities {
                AiCapabilities::default()
            }
            async fn explain(&self, _req: &ExplainRequest) -> AiResult<AiResponse> {
                std::future::pending().await
            }
            async fn suggest_sql(&self, _req: &SuggestRequest) -> AiResult<AiResponse> {
                unreachable!()
            }
        }
        let provider: Arc<dyn AiProvider> = Arc::new(PendingAtomicProvider);
        let (tx, rx) = mpsc::channel::<Reply>();
        let token = CancellationToken::new();
        let token_clone = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            token_clone.cancel();
        });
        run_explain_atomic(Some(provider), explain_req(), token, tx, ctx()).await;
        let replies = drain(&rx);
        assert_eq!(replies.len(), 1);
        assert!(matches!(&replies[0], Reply::AiCancelled));
    }

    #[tokio::test]
    async fn atomic_success_short_circuits_cancel_race_into_responded() {
        let provider: Arc<dyn AiProvider> = Arc::new(StubProvider::atomic_ok("ok", 1, 2));
        let (tx, rx) = mpsc::channel::<Reply>();
        run_explain_atomic(
            Some(provider),
            explain_req(),
            CancellationToken::new(),
            tx,
            ctx(),
        )
        .await;
        let replies = drain(&rx);
        assert_eq!(replies.len(), 1);
        match &replies[0] {
            Reply::AiResponded {
                text,
                tokens_in,
                tokens_out,
            } => {
                assert_eq!(text, "ok");
                assert_eq!(*tokens_in, 1);
                assert_eq!(*tokens_out, 2);
            }
            other => panic!("expected AiResponded, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn atomic_suggest_success_short_circuits_cancel_race_into_responded() {
        let provider: Arc<dyn AiProvider> = Arc::new(StubProvider::atomic_ok("sql", 3, 4));
        let (tx, rx) = mpsc::channel::<Reply>();
        run_suggest_atomic(
            Some(provider),
            suggest_req(),
            CancellationToken::new(),
            tx,
            ctx(),
        )
        .await;
        let replies = drain(&rx);
        assert!(matches!(
            &replies[0],
            Reply::AiResponded { text, .. } if text == "sql"
        ));
    }

    // ---- no-provider gate ---------------------------------------------

    #[tokio::test]
    async fn streaming_without_provider_surfaces_configuration_failure() {
        let (tx, rx) = mpsc::channel::<Reply>();
        run_explain_stream(None, explain_req(), CancellationToken::new(), tx, ctx()).await;
        let replies = drain(&rx);
        assert_eq!(replies.len(), 1);
        assert!(matches!(
            &replies[0],
            Reply::AiFailed {
                error: AiError::Configuration(_)
            }
        ));
    }

    #[tokio::test]
    async fn atomic_without_provider_surfaces_configuration_failure() {
        let (tx, rx) = mpsc::channel::<Reply>();
        run_explain_atomic(None, explain_req(), CancellationToken::new(), tx, ctx()).await;
        let replies = drain(&rx);
        assert!(matches!(
            &replies[0],
            Reply::AiFailed {
                error: AiError::Configuration(_)
            }
        ));
    }
}
