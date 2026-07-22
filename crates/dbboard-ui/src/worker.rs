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
use dbboard_core::{plan_dump, DatabaseAdapter, DbError, TableInfo, TableSchema};
use eframe::egui;
use futures_util::StreamExt;
use tokio::sync::mpsc as tmpsc;
use tokio::sync::Semaphore;
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
use crate::edit::dialect_for_adapter_id;
use crate::{backup, Command, Reply};

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

/// Live-adapter view for `Command::PrefetchSchema` (ADR-0028
/// Decision 9). Same injection pattern as [`ConnectionSwitcher`] /
/// [`AiProviderSwitcher`]: the desktop binary implements it over the
/// server's `AppState` (whose `current_adapter()` is the same
/// per-request snapshot the HTTP handlers use), so the fan-out sees the
/// adapter that is live *at dispatch time* — a connection switch
/// between commands is picked up on the next prefetch. Kept narrow on
/// purpose: the worker only ever needs a snapshot, never the slot.
pub trait SchemaSource: Send + Sync + 'static {
    /// Snapshot the currently-live adapter. The returned `Arc` is
    /// stable for the duration of one fan-out; a concurrent swap
    /// affects the *next* snapshot, mirroring ADR-0020's per-request
    /// capture.
    fn current_adapter(&self) -> Arc<dyn DatabaseAdapter>;
}

/// Concurrency cap for the `describe_table` fan-out (ADR-0028
/// Decision 9): bounded so a 200-table schema cannot exhaust the
/// adapter's connection pool or hammer a serverless endpoint.
const MAX_CONCURRENT_DESCRIBES: usize = 8;

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
// Same rationale as `connect`: one more injected handle per in-process
// concern (ADR-0020 switcher, ADR-0025 ai_switcher, ADR-0028
// schema_source); the queued struct-builder refactor will collapse
// these together.
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_worker(
    base_url: String,
    cmd_rx: Receiver<Command>,
    reply_tx: Sender<Reply>,
    egui_ctx: egui::Context,
    switcher: Arc<dyn ConnectionSwitcher>,
    ai_switcher: Arc<dyn AiProviderSwitcher>,
    ai_provider_slot: AiProviderSlot,
    schema_source: Option<Arc<dyn SchemaSource>>,
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
                schema_source,
            );
        })
        .expect("spawn dbboard-http-worker thread");
}

#[allow(clippy::too_many_arguments)]
fn run_worker(
    base_url: &str,
    cmd_rx: Receiver<Command>,
    reply_tx: Sender<Reply>,
    egui_ctx: egui::Context,
    switcher: Arc<dyn ConnectionSwitcher>,
    ai_switcher: Arc<dyn AiProviderSwitcher>,
    ai_provider_slot: AiProviderSlot,
    schema_source: Option<Arc<dyn SchemaSource>>,
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
        schema_source,
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
    schema_source: Option<Arc<dyn SchemaSource>>,
) {
    // Single-slot in-flight AI cancel handle. ADR-0026's UI gates every
    // AI command on `busy`, so at most one AI request is in flight at
    // any time — a HashMap is unnecessary. A stale Some value left
    // after a task completes naturally is harmless: the next AI command
    // overwrites it, and a CancelAiRequest on it just `.cancel()`s a
    // token nobody is listening on.
    let mut in_flight: Option<CancellationToken> = None;
    // Separate single-slot cancel handle for the in-flight backup
    // (ADR-0049 slice e). A backup and an AI request can be in flight at
    // once, so they cannot share the AI slot; the UI still gates to at
    // most one backup at a time, so one slot suffices here too.
    let mut backup_in_flight: Option<CancellationToken> = None;
    while let Some(cmd) = cmd_rx.recv().await {
        handle_command(
            cmd,
            &mut in_flight,
            &mut backup_in_flight,
            base_url,
            &reply_tx,
            &egui_ctx,
            &http,
            switcher.as_ref(),
            ai_switcher.as_ref(),
            &ai_provider_slot,
            schema_source.as_deref(),
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
    backup_in_flight: &mut Option<CancellationToken>,
    base_url: &str,
    reply_tx: &Sender<Reply>,
    egui_ctx: &egui::Context,
    http: &reqwest::Client,
    switcher: &dyn ConnectionSwitcher,
    ai_switcher: &dyn AiProviderSwitcher,
    ai_provider_slot: &AiProviderSlot,
    schema_source: Option<&dyn SchemaSource>,
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
        // are still drained. ADR-0027 Slice b: `identity` snapshotted
        // at spawn time by `spawn_ai_task` and stamped on every
        // terminal reply — spawn-time identity is the contract because
        // the AI provider slot can swap mid-request.
        Command::AiExplainStream { sql, dialect } => {
            spawn_ai_task(
                in_flight,
                ai_provider_slot,
                reply_tx,
                egui_ctx,
                |p, id, t, tx, c| {
                    Box::pin(run_explain_stream(
                        p,
                        ExplainRequest { sql, dialect },
                        id,
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
            full_schema,
        } => {
            spawn_ai_task(
                in_flight,
                ai_provider_slot,
                reply_tx,
                egui_ctx,
                |p, id, t, tx, c| {
                    Box::pin(run_suggest_stream(
                        p,
                        SuggestRequest {
                            prompt,
                            dialect,
                            schema,
                            full_schema,
                        },
                        id,
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
                |p, id, t, tx, c| {
                    Box::pin(run_explain_atomic(
                        p,
                        ExplainRequest { sql, dialect },
                        id,
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
            full_schema,
        } => {
            spawn_ai_task(
                in_flight,
                ai_provider_slot,
                reply_tx,
                egui_ctx,
                |p, id, t, tx, c| {
                    Box::pin(run_suggest_atomic(
                        p,
                        SuggestRequest {
                            prompt,
                            dialect,
                            schema,
                            full_schema,
                        },
                        id,
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
        // ADR-0028 Decision 9: describe_table fan-out. Awaited inline —
        // the panel keeps its Send gate up (busy) while the prefetch is
        // in flight, so no other AI command competes for the loop, and
        // individual describes are short. In-process, never HTTP.
        Command::PrefetchSchema { tables } => {
            let reply = match schema_source {
                Some(source) => {
                    let adapter = source.current_adapter();
                    let (schemas, errors) = prefetch_schemas(adapter, tables).await;
                    Reply::SchemaPrefetched { schemas, errors }
                }
                // Defence-in-depth: the panel only issues PrefetchSchema
                // when db_has_describe_table() said yes, which requires
                // a wired source. Answer totally anyway so a stray
                // command cannot strand the panel in its busy state.
                None => Reply::SchemaPrefetched {
                    schemas: Vec::new(),
                    errors: tables
                        .into_iter()
                        .map(|t| (t, "no schema source wired".to_string()))
                        .collect(),
                },
            };
            let _ = reply_tx.send(reply);
            egui_ctx.request_repaint();
        }
        // ADR-0031: single-table describe for the structure tab. Same
        // in-process describe_table path as PrefetchSchema, scoped to one
        // table and awaited inline (a single describe is short).
        Command::DescribeTable { table } => {
            let result = match schema_source {
                Some(source) => source.current_adapter().describe_table(&table).await,
                None => Err(DbError::Capability(
                    "describe_table unavailable on this connection".to_string(),
                )),
            };
            let _ = reply_tx.send(Reply::TableDescribed { table, result });
            egui_ctx.request_repaint();
        }
        // ADR-0049 slice e: backup preflight. Awaited inline — it is one
        // list_tables plus a COUNT(*) per table, and the UI keeps its
        // Backup button disabled until the plan lands. In-process, never
        // HTTP.
        Command::PlanBackup => {
            let result = match schema_source {
                Some(source) => {
                    let adapter = source.current_adapter();
                    match dialect_for_adapter_id(adapter.id()) {
                        Some(dialect) => plan_dump(adapter.as_ref(), dialect).await,
                        None => Err(unsupported_backup_error()),
                    }
                }
                None => Err(unsupported_backup_error()),
            };
            let _ = reply_tx.send(Reply::BackupPlanned { result });
            egui_ctx.request_repaint();
        }
        // ADR-0049 slice e: run the dump on a spawned task so the loop
        // keeps draining commands (notably CancelBackup) while it pages.
        // The dialect is re-derived from the live adapter rather than
        // carried from PlanBackup, so a connection switch between the two
        // cannot desync it.
        Command::StartBackup { path, plan } => {
            let target = schema_source.and_then(|source| {
                let adapter = source.current_adapter();
                dialect_for_adapter_id(adapter.id()).map(|dialect| (adapter, dialect))
            });
            if let Some((adapter, dialect)) = target {
                let token = CancellationToken::new();
                *backup_in_flight = Some(token.clone());
                tokio::spawn(backup::run_backup(
                    adapter,
                    dialect,
                    plan,
                    path,
                    token,
                    reply_tx.clone(),
                    egui_ctx.clone(),
                ));
            } else {
                let _ = reply_tx.send(Reply::BackupFailed {
                    message: unsupported_backup_error().message().to_owned(),
                });
                egui_ctx.request_repaint();
            }
        }
        // ADR-0049 Decision 9: cancel the in-flight backup, if any. The
        // dump task observes the token at its next boundary and still
        // reports a (cancelled) BackupComplete on its way out.
        Command::CancelBackup => {
            if let Some(token) = backup_in_flight.take() {
                token.cancel();
            }
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

/// The error a backup command answers with when the active connection
/// cannot be dumped: either no schema source is wired, or its adapter id
/// maps to no SQL dialect (ADR-0049). Kept as one helper so the preflight
/// and the start paths report identically.
fn unsupported_backup_error() -> DbError {
    DbError::Capability("backup is unavailable on this connection".to_string())
}

/// Fan out `describe_table` over `tables`, at most
/// [`MAX_CONCURRENT_DESCRIBES`] in flight at once (ADR-0028
/// Decision 9). `join_all` preserves input order on both the success
/// and the error side, so the downstream prompt rendering is
/// deterministic regardless of per-table completion timing.
pub(crate) async fn prefetch_schemas(
    adapter: Arc<dyn DatabaseAdapter>,
    tables: Vec<TableInfo>,
) -> (Vec<TableSchema>, Vec<(TableInfo, String)>) {
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_DESCRIBES));
    let describes = tables.into_iter().map(|table| {
        let adapter = Arc::clone(&adapter);
        let semaphore = Arc::clone(&semaphore);
        async move {
            // `acquire_owned` fails only when the semaphore is closed,
            // which never happens here (we never call `close`). Mapped
            // to a per-table error rather than unwrapped so the fan-out
            // stays total even if that invariant ever breaks.
            let result = match semaphore.acquire_owned().await {
                Ok(_permit) => adapter
                    .describe_table(&table)
                    .await
                    .map_err(|e| e.message().to_string()),
                Err(_) => Err("describe_table scheduling failed".to_string()),
            };
            (table, result)
        }
    });
    let mut schemas = Vec::new();
    let mut errors = Vec::new();
    for (table, result) in futures_util::future::join_all(describes).await {
        match result {
            Ok(schema) => schemas.push(schema),
            Err(message) => errors.push((table, message)),
        }
    }
    (schemas, errors)
}

/// Wire up an AI task: snapshot the provider AND its identity, install
/// a fresh cancel token in `in_flight`, and `tokio::spawn` the runner
/// the caller builds. The runner is built lazily via the closure so
/// each variant can own its request payload.
///
/// ADR-0027 Slice b: `identity` is snapshotted here — the exact same
/// tuple gets stamped onto every terminal reply the runner emits
/// (`AiResponded` / `AiStreamComplete` / `AiFailed` / `AiCancelled`).
/// A `SwitchAiProvider` command reaching the main loop after this
/// spawn changes the *next* request's identity, not this one — that's
/// the spawn-time-identity contract from ADR-0027 §Implementation.
fn spawn_ai_task<F>(
    in_flight: &mut Option<CancellationToken>,
    ai_provider_slot: &AiProviderSlot,
    reply_tx: &Sender<Reply>,
    egui_ctx: &egui::Context,
    runner: F,
) where
    F: FnOnce(
        Option<Arc<dyn AiProvider>>,
        (String, String),
        CancellationToken,
        Sender<Reply>,
        egui::Context,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>,
{
    let token = CancellationToken::new();
    *in_flight = Some(token.clone());
    let provider = snapshot_provider(ai_provider_slot);
    let identity = snapshot_identity(provider.as_deref());
    let reply_tx = reply_tx.clone();
    let ctx = egui_ctx.clone();
    let fut = runner(provider, identity, token, reply_tx, ctx);
    tokio::spawn(fut);
}

fn snapshot_provider(slot: &AiProviderSlot) -> Option<Arc<dyn AiProvider>> {
    // Snapshot per dispatch so a swap between commands becomes visible
    // on the next one. A poisoned lock is recovered into the inner
    // value because a writer panicking mid-swap leaves the slot in a
    // valid (Some or None) state.
    slot.read().unwrap_or_else(PoisonError::into_inner).clone()
}

/// Snapshot `(provider_id, model_id)` from the provider, or the default
/// `("unknown", "")` when the slot is empty. Returned as owned strings
/// so the tuple can travel through `tokio::spawn` (the `AiProvider`
/// trait's `identity()` returns a borrow into the provider — fine to
/// hold in the same task, but we clone at the boundary so no lifetime
/// leaks into the spawned future's signature).
fn snapshot_identity(provider: Option<&(dyn AiProvider + '_)>) -> (String, String) {
    match provider {
        Some(p) => {
            let (id, model) = p.identity();
            (id.to_string(), model.to_string())
        }
        None => ("unknown".into(), String::new()),
    }
}

/// ADR-0026 Decision 10: atomic explain through the cancel race.
/// ADR-0027 Slice b: `identity` is the spawn-time snapshot; every
/// terminal reply carries it verbatim.
pub(crate) async fn run_explain_atomic(
    provider: Option<Arc<dyn AiProvider>>,
    req: ExplainRequest,
    identity: (String, String),
    token: CancellationToken,
    reply_tx: Sender<Reply>,
    egui_ctx: egui::Context,
) {
    let Some(provider) = provider else {
        let _ = reply_tx.send(no_provider_failure(&identity));
        egui_ctx.request_repaint();
        return;
    };
    let reply = tokio::select! {
        biased;
        () = token.cancelled() => cancelled_reply(&identity),
        result = provider.explain(&req) => ai_reply(result, &identity),
    };
    let _ = reply_tx.send(reply);
    egui_ctx.request_repaint();
}

/// ADR-0026 Decision 10: atomic `suggest_sql` through the cancel race.
pub(crate) async fn run_suggest_atomic(
    provider: Option<Arc<dyn AiProvider>>,
    req: SuggestRequest,
    identity: (String, String),
    token: CancellationToken,
    reply_tx: Sender<Reply>,
    egui_ctx: egui::Context,
) {
    let Some(provider) = provider else {
        let _ = reply_tx.send(no_provider_failure(&identity));
        egui_ctx.request_repaint();
        return;
    };
    let reply = tokio::select! {
        biased;
        () = token.cancelled() => cancelled_reply(&identity),
        result = provider.suggest_sql(&req) => ai_reply(result, &identity),
    };
    let _ = reply_tx.send(reply);
    egui_ctx.request_repaint();
}

/// ADR-0026 Decisions 5/6/12: open a streaming explain, race the
/// per-chunk `next()` against the cancel token, and forward each
/// chunk over the reply channel. ADR-0027 Slice b: `identity` stamps
/// every terminal reply.
pub(crate) async fn run_explain_stream(
    provider: Option<Arc<dyn AiProvider>>,
    req: ExplainRequest,
    identity: (String, String),
    token: CancellationToken,
    reply_tx: Sender<Reply>,
    egui_ctx: egui::Context,
) {
    let Some(provider) = provider else {
        let _ = reply_tx.send(no_provider_failure(&identity));
        egui_ctx.request_repaint();
        return;
    };
    let open = tokio::select! {
        biased;
        () = token.cancelled() => {
            let _ = reply_tx.send(cancelled_reply(&identity));
            egui_ctx.request_repaint();
            return;
        }
        result = provider.stream_explain(&req) => result,
    };
    forward_stream(open, identity, token, reply_tx, egui_ctx).await;
}

/// ADR-0026 Decisions 5/6/12: streaming `suggest_sql`, same shape as
/// [`run_explain_stream`].
pub(crate) async fn run_suggest_stream(
    provider: Option<Arc<dyn AiProvider>>,
    req: SuggestRequest,
    identity: (String, String),
    token: CancellationToken,
    reply_tx: Sender<Reply>,
    egui_ctx: egui::Context,
) {
    let Some(provider) = provider else {
        let _ = reply_tx.send(no_provider_failure(&identity));
        egui_ctx.request_repaint();
        return;
    };
    let open = tokio::select! {
        biased;
        () = token.cancelled() => {
            let _ = reply_tx.send(cancelled_reply(&identity));
            egui_ctx.request_repaint();
            return;
        }
        result = provider.stream_suggest_sql(&req) => result,
    };
    forward_stream(open, identity, token, reply_tx, egui_ctx).await;
}

/// Drive an opened [`AiStream`] to completion, forwarding each event
/// onto the reply channel and racing every `next()` against the
/// cancel token. Cumulative token counts are tracked locally so the
/// terminal `AiStreamComplete` carries the last-known values even when
/// the final `message_delta` does not repeat them.
async fn forward_stream(
    opened: dbboard_ai::AiResult<AiStream>,
    identity: (String, String),
    token: CancellationToken,
    reply_tx: Sender<Reply>,
    egui_ctx: egui::Context,
) {
    let mut stream = match opened {
        Ok(s) => s,
        Err(error) => {
            let _ = reply_tx.send(failed_reply(error, &identity));
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
                let _ = reply_tx.send(cancelled_reply(&identity));
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
                let _ = reply_tx.send(complete_reply(
                    last_tokens_in,
                    last_tokens_out,
                    StopReason::EndTurn,
                    &identity,
                ));
                egui_ctx.request_repaint();
                return;
            }
            Some(Err(error)) => {
                let _ = reply_tx.send(failed_reply(error, &identity));
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
                    let _ = reply_tx.send(complete_reply(
                        last_tokens_in,
                        last_tokens_out,
                        stop_reason,
                        &identity,
                    ));
                    egui_ctx.request_repaint();
                    return;
                }
                StreamEvent::Error(error) => {
                    let _ = reply_tx.send(failed_reply(error, &identity));
                    egui_ctx.request_repaint();
                    return;
                }
            },
        }
    }
}

fn complete_reply(
    tokens_in: u32,
    tokens_out: u32,
    stop_reason: StopReason,
    identity: &(String, String),
) -> Reply {
    Reply::AiStreamComplete {
        tokens_in,
        tokens_out,
        stop_reason,
        provider: identity.0.clone(),
        model: identity.1.clone(),
    }
}

fn failed_reply(error: AiError, identity: &(String, String)) -> Reply {
    Reply::AiFailed {
        error,
        provider: identity.0.clone(),
        model: identity.1.clone(),
    }
}

fn ai_reply(
    result: dbboard_ai::AiResult<dbboard_ai::AiResponse>,
    identity: &(String, String),
) -> Reply {
    match result {
        Ok(resp) => Reply::AiResponded {
            text: resp.text,
            tokens_in: resp.tokens_in,
            tokens_out: resp.tokens_out,
            provider: identity.0.clone(),
            model: identity.1.clone(),
        },
        Err(error) => failed_reply(error, identity),
    }
}

fn cancelled_reply(identity: &(String, String)) -> Reply {
    Reply::AiCancelled {
        provider: identity.0.clone(),
        model: identity.1.clone(),
    }
}

fn no_provider_failure(identity: &(String, String)) -> Reply {
    failed_reply(
        AiError::Configuration("no AI provider configured; set DBBOARD_ANTHROPIC_API_KEY".into()),
        identity,
    )
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
                // Worker never came up — no provider was ever snapshotted.
                // Stamp with the sentinel identity so slice (c)'s history
                // record path treats it like any other AiFailed.
                provider: "unknown".into(),
                model: String::new(),
            },
            // ADR-0026 Decision 5: a cancel arriving on the fatal path
            // is acknowledged with AiCancelled so the panel exits busy
            // — the request the user wanted to cancel was never
            // dispatched in the first place.
            Command::CancelAiRequest => Reply::AiCancelled {
                provider: "unknown".into(),
                model: String::new(),
            },
            Command::SwitchAiProvider { .. } => Reply::AiProviderSwitchFailed {
                reason: format!("ai worker unavailable: {}", err.message()),
            },
            // All-errors reply keeps the panel's prefetch state machine
            // moving: it fires the pending Suggest with an empty
            // full_schema, whose AiFailed terminal (also produced here)
            // then resets the panel.
            Command::PrefetchSchema { tables } => Reply::SchemaPrefetched {
                schemas: Vec::new(),
                errors: tables
                    .into_iter()
                    .map(|t| (t, err.message().to_string()))
                    .collect(),
            },
            // ADR-0031: the structure tab gets the fatal error verbatim so
            // it renders the failure instead of spinning forever.
            Command::DescribeTable { table } => Reply::TableDescribed {
                table,
                result: Err(err.clone()),
            },
            // ADR-0049: the backup preflight/start surface the fatal error
            // so their modal reports it instead of hanging on a plan that
            // never arrives.
            Command::PlanBackup => Reply::BackupPlanned {
                result: Err(err.clone()),
            },
            Command::StartBackup { .. } => Reply::BackupFailed {
                message: err.message().to_string(),
            },
            // Nothing is in flight on the fatal path, so a cancel needs no
            // reply — the UI never entered a running state.
            Command::CancelBackup => continue,
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
        prefetch_schemas, run_explain_atomic, run_explain_stream, run_suggest_atomic,
        run_suggest_stream, AiProviderSwitcher, ConnectionSwitcher, MAX_CONCURRENT_DESCRIBES,
    };
    use crate::Reply;
    use dbboard_ai::{
        AiCapabilities, AiError, AiProvider, AiResponse, AiResult, AiStream, ExplainRequest,
        StopReason, StreamEvent, SuggestRequest,
    };
    use dbboard_core::{
        Capabilities, ColumnInfo, DatabaseAdapter, DbError, DbResult, QueryResult, TableInfo,
        TableSchema,
    };
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
                    provider: "stub".into(),
                    model: "stub-model".into(),
                }),
                suggest_response: Ok(AiResponse {
                    text: "unused".into(),
                    tokens_in: 0,
                    tokens_out: 0,
                    provider: "stub".into(),
                    model: "stub-model".into(),
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
                    provider: "stub".into(),
                    model: "stub-model".into(),
                }),
                suggest_response: Ok(AiResponse {
                    text: text.into(),
                    tokens_in,
                    tokens_out,
                    provider: "stub".into(),
                    model: "stub-model".into(),
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
                    provider: resp.provider.clone(),
                    model: resp.model.clone(),
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
        fn identity(&self) -> (&'static str, &str) {
            ("stub", "stub-model")
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

    /// ADR-0027 Slice b: mirrors the `snapshot_identity` result the
    /// worker takes at spawn time from `StubProvider::identity()`.
    fn stub_identity() -> (String, String) {
        ("stub".into(), "stub-model".into())
    }

    /// Identity sentinel used by `spawn_ai_task` when the AI slot is
    /// empty. Tests exercising the no-provider gate use this so the
    /// terminal `AiFailed` still carries a well-formed identity.
    fn sentinel_identity() -> (String, String) {
        ("unknown".into(), String::new())
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
            full_schema: None,
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
        run_explain_stream(
            Some(provider),
            explain_req(),
            stub_identity(),
            token,
            tx,
            ctx(),
        )
        .await;
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
        // Terminal complete with the last-known cumulative counts +
        // spawn-time identity (ADR-0027 Slice b).
        match &replies[4] {
            Reply::AiStreamComplete {
                tokens_in,
                tokens_out,
                stop_reason,
                provider,
                model,
            } => {
                assert_eq!(*tokens_in, 11);
                assert_eq!(*tokens_out, 7);
                assert!(matches!(stop_reason, StopReason::EndTurn));
                assert_eq!(provider, "stub");
                assert_eq!(model, "stub-model");
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
            stub_identity(),
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
            stub_identity(),
            CancellationToken::new(),
            tx,
            ctx(),
        )
        .await;
        let replies = drain(&rx);
        assert_eq!(replies.len(), 3, "MessageStart + TextDelta + AiFailed");
        assert!(
            matches!(&replies[2], Reply::AiFailed { error, provider, model }
                if matches!(error, AiError::Provider(s) if s == "overloaded")
                && provider == "stub" && model == "stub-model")
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
            stub_identity(),
            CancellationToken::new(),
            tx,
            ctx(),
        )
        .await;
        let replies = drain(&rx);
        assert_eq!(replies.len(), 2);
        assert!(
            matches!(&replies[1], Reply::AiFailed { error, .. } if matches!(error, AiError::Network(s) if s == "conn reset"))
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
            stub_identity(),
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
        run_explain_stream(
            Some(provider),
            explain_req(),
            ("pending".into(), String::new()),
            token,
            tx,
            ctx(),
        )
        .await;
        let replies = drain(&rx);
        assert_eq!(replies.len(), 1);
        assert!(matches!(
            &replies[0],
            Reply::AiCancelled { provider, model }
                if provider == "pending" && model.is_empty()
        ));
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
        run_explain_atomic(
            Some(provider),
            explain_req(),
            ("pending-atomic".into(), String::new()),
            token,
            tx,
            ctx(),
        )
        .await;
        let replies = drain(&rx);
        assert_eq!(replies.len(), 1);
        assert!(matches!(
            &replies[0],
            Reply::AiCancelled { provider, model }
                if provider == "pending-atomic" && model.is_empty()
        ));
    }

    #[tokio::test]
    async fn atomic_success_short_circuits_cancel_race_into_responded() {
        let provider: Arc<dyn AiProvider> = Arc::new(StubProvider::atomic_ok("ok", 1, 2));
        let (tx, rx) = mpsc::channel::<Reply>();
        run_explain_atomic(
            Some(provider),
            explain_req(),
            stub_identity(),
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
                provider,
                model,
            } => {
                assert_eq!(text, "ok");
                assert_eq!(*tokens_in, 1);
                assert_eq!(*tokens_out, 2);
                assert_eq!(provider, "stub");
                assert_eq!(model, "stub-model");
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
            stub_identity(),
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
        run_explain_stream(
            None,
            explain_req(),
            sentinel_identity(),
            CancellationToken::new(),
            tx,
            ctx(),
        )
        .await;
        let replies = drain(&rx);
        assert_eq!(replies.len(), 1);
        assert!(matches!(
            &replies[0],
            Reply::AiFailed {
                error: AiError::Configuration(_),
                provider,
                model,
            } if provider == "unknown" && model.is_empty()
        ));
    }

    // ---- ADR-0028: PrefetchSchema fan-out ------------------------------

    /// Adapter stub for the describe fan-out: succeeds with a one-column
    /// schema unless the table name starts with `bad`, and tracks the
    /// high-water mark of concurrent `describe_table` calls.
    struct DescribeAdapter {
        current: AtomicUsize,
        max_seen: AtomicUsize,
        delay: Duration,
    }

    impl DescribeAdapter {
        fn new(delay: Duration) -> Self {
            Self {
                current: AtomicUsize::new(0),
                max_seen: AtomicUsize::new(0),
                delay,
            }
        }
    }

    #[async_trait::async_trait]
    impl DatabaseAdapter for DescribeAdapter {
        fn id(&self) -> &'static str {
            "describe-stub"
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities {
                has_describe_table: true,
                ..Capabilities::default()
            }
        }
        async fn ping(&self) -> DbResult<()> {
            Ok(())
        }
        async fn list_tables(&self) -> DbResult<Vec<TableInfo>> {
            Ok(Vec::new())
        }
        async fn query(&self, _sql: &str) -> DbResult<QueryResult> {
            Ok(QueryResult::empty())
        }
        async fn describe_table(&self, table: &TableInfo) -> DbResult<TableSchema> {
            let now = self.current.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_seen.fetch_max(now, Ordering::SeqCst);
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            self.current.fetch_sub(1, Ordering::SeqCst);
            if table.name.starts_with("bad") {
                return Err(DbError::Schema(format!("no such table: {}", table.name)));
            }
            Ok(TableSchema {
                table: table.clone(),
                columns: vec![ColumnInfo {
                    name: "id".into(),
                    declared_type: Some("INTEGER".into()),
                    nullable: false,
                    primary_key: true,
                    ordinal: 1,
                    default_value: None,
                }],
                primary_key: vec!["id".into()],
            })
        }
    }

    #[tokio::test]
    async fn prefetch_schemas_partitions_successes_and_errors_in_input_order() {
        let adapter: Arc<dyn DatabaseAdapter> = Arc::new(DescribeAdapter::new(Duration::ZERO));
        let tables = vec![
            TableInfo::unqualified("alpha"),
            TableInfo::unqualified("bad_one"),
            TableInfo::unqualified("beta"),
            TableInfo::unqualified("bad_two"),
            TableInfo::unqualified("gamma"),
        ];
        let (schemas, errors) = prefetch_schemas(adapter, tables).await;

        let names: Vec<&str> = schemas.iter().map(|s| s.table.name.as_str()).collect();
        assert_eq!(names, ["alpha", "beta", "gamma"], "input order preserved");
        let failed: Vec<&str> = errors.iter().map(|(t, _)| t.name.as_str()).collect();
        assert_eq!(failed, ["bad_one", "bad_two"]);
        assert!(
            errors[0].1.contains("no such table: bad_one"),
            "error message travels with the table: {}",
            errors[0].1
        );
    }

    #[tokio::test]
    async fn prefetch_schemas_with_no_tables_returns_empty_partitions() {
        let adapter: Arc<dyn DatabaseAdapter> = Arc::new(DescribeAdapter::new(Duration::ZERO));
        let (schemas, errors) = prefetch_schemas(adapter, Vec::new()).await;
        assert!(schemas.is_empty());
        assert!(errors.is_empty());
    }

    #[tokio::test]
    async fn prefetch_schemas_caps_concurrent_describes_at_the_semaphore_limit() {
        // 3x the cap worth of tables, each holding its permit across a
        // sleep, so an uncapped join_all would show ~24 concurrent
        // calls. The high-water mark must never exceed the cap.
        let adapter = Arc::new(DescribeAdapter::new(Duration::from_millis(10)));
        let tables: Vec<TableInfo> = (0..MAX_CONCURRENT_DESCRIBES * 3)
            .map(|i| TableInfo::unqualified(format!("t{i}")))
            .collect();
        let expected = tables.len();
        let (schemas, errors) =
            prefetch_schemas(Arc::clone(&adapter) as Arc<dyn DatabaseAdapter>, tables).await;

        assert_eq!(schemas.len(), expected);
        assert!(errors.is_empty());
        let max_seen = adapter.max_seen.load(Ordering::SeqCst);
        assert!(
            max_seen <= MAX_CONCURRENT_DESCRIBES,
            "semaphore must cap concurrency: saw {max_seen}, cap {MAX_CONCURRENT_DESCRIBES}"
        );
        assert!(
            max_seen > 1,
            "fan-out must actually run describes concurrently, saw {max_seen}"
        );
    }

    #[tokio::test]
    async fn atomic_without_provider_surfaces_configuration_failure() {
        let (tx, rx) = mpsc::channel::<Reply>();
        run_explain_atomic(
            None,
            explain_req(),
            sentinel_identity(),
            CancellationToken::new(),
            tx,
            ctx(),
        )
        .await;
        let replies = drain(&rx);
        assert!(matches!(
            &replies[0],
            Reply::AiFailed {
                error: AiError::Configuration(_),
                provider,
                model,
            } if provider == "unknown" && model.is_empty()
        ));
    }
}
