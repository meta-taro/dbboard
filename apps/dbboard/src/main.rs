//! dbboard desktop binary entry point.
//!
//! The binary boots an in-process loopback HTTP server (`dbboard-server`)
//! and points the egui UI (`dbboard-ui`) at it. The server owns the
//! database adapter and resolves which backend to connect from the
//! environment plus the user's local connection store (see
//! [`dbboard_server::backend_config_from_env_and_store`] and ADR-0013).
//! The UI is a pure HTTP client. This keeps the desktop app on the same
//! API contract as the dbboard-web sibling (ADR-0009).
//!
//! Two runtimes coexist without nesting: this `main` owns a multi-thread
//! tokio runtime that drives the server, while the UI's HTTP worker runs
//! a separate current-thread runtime on its own thread. The UI thread
//! itself never blocks on I/O.
//!
//! Startup resolves config in this order: env vars > `DBBOARD_CONNECTION`
//! id > single-entry auto-select > Turso `:memory:`. Failures (a missing
//! id, a missing keyring entry) abort startup loudly rather than
//! silently swapping in a different backend.
//!
//! The AI provider follows an analogous but optional precedence
//! (ADR-0023 + ADR-0025): env (`DBBOARD_ANTHROPIC_API_KEY`) >
//! `ai-providers.toml` active id > none. AI is opt-in, so a failure on
//! any branch degrades to no provider rather than aborting startup. A
//! runtime switch performed through [`DesktopAiSwitcher`] persists the
//! new active id to TOML *and* swaps the live provider slot in-process,
//! so the worker thread sees the change on the next AI command.
//!
//! Locale resolution (ADR-0015) runs here too: `DBBOARD_LANG` > OS
//! locale > `en`. The binary also registers an OS CJK font into the
//! egui font stack so `ja` / `ko` / `zh-CN` / `zh-TW` users do not see
//! tofu — egui's bundled Ubuntu-Light covers Latin + Cyrillic but no
//! CJK ranges.

// Suppress the console window on Windows release builds: this is a GUI
// app, so a flashing terminal behind it is pure noise for end users.
// Debug builds keep the console so `println!`/panic traces stay visible
// during development. No-op on non-Windows targets.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use dbboard_ai::{AiError, AiProvider};
use dbboard_anthropic::AnthropicProvider;
use dbboard_config::store::{default_history_path, default_path, load_or_empty};
use dbboard_config::{
    default_ai_providers_path, default_annotations_path, default_ui_settings_path,
    load_ui_settings, save_ui_settings, secure_fs, AiProviderKind, AiSettingsAdmin,
    AnnotationsAdmin, ConnectionAdmin, ConnectionFile, KeyringStore, SecretStore, ThemePreference,
    UiSettingsFile,
};
use dbboard_i18n::{t, t_args};
use dbboard_server::{
    backend_config_for_entry, backend_config_from_env_and_store, build_adapter,
    resolved_connection_label, serve, swap_backend, AppState, BackendConfig, ServerError,
};
use dbboard_ui::{
    AiProviderSlot, AiProviderSwitcher, AiSettingsView, ConnectionSwitcher, ConnectionsView,
    DatabaseAdapter, DbError, DbboardApp, PersistentHistoryStore, SchemaSource, DEFAULT_CAPACITY,
};
use time::format_description::well_known::Rfc3339;

mod update_check;

/// Locales offered by the runtime language switcher (ADR-0022). The
/// tag must match a shipped `dbboard-i18n` locale folder; the second
/// element is the locale's name written *in itself* (`日本語` for
/// Japanese, `한국어` for Korean) and is intentionally **not**
/// translated — a user who landed in the wrong locale at startup must
/// be able to spot their language without already reading it.
///
/// Order is fixed (Tier 1 then Tier 2 from ADR-0015) so the submenu
/// does not reshuffle as the active locale changes.
const SUPPORTED_LOCALES: &[(&str, &str)] = &[
    ("en", "English"),
    ("ja", "日本語"),
    ("ko", "한국어"),
    ("zh-CN", "中文 (简体)"),
    ("zh-TW", "中文 (繁體)"),
    ("de", "Deutsch"),
    ("fr", "Français"),
    ("es", "Español"),
    ("pt-BR", "Português (Brasil)"),
    ("ru", "Русский"),
    ("it", "Italiano"),
];
use time::OffsetDateTime;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Resolve and apply the user's locale before any UI string is read.
    // A failure here is non-fatal: we keep going with the en fallback so
    // a malformed .ftl in a future locale can never brick startup.
    if let Err(e) = dbboard_i18n::init(None) {
        eprintln!("dbboard: locale init failed, falling back to en: {e}");
    }

    // Share a single secret-store handle between startup (server backend
    // resolution) and runtime (connection management UI) so a token
    // added through the UI is immediately visible to subsequent reads.
    let secrets: Arc<dyn SecretStore> = Arc::new(KeyringStore::new());

    // Best-effort config-dir resolution: if the OS reports no per-user
    // config dir at all, treat the store as empty rather than aborting.
    // The env-var path still works in that case (CI, headless tests).
    // When the dir does resolve, we build a ConnectionAdmin so the UI
    // can mutate the same file the server resolved its backend from.
    let (file, admin) = match default_path() {
        Ok(path) => {
            // ADR-0024: warn (don't abort) when the per-user config dir
            // resolves under a cloud-sync vendor folder (OneDrive Known
            // Folder Move, iCloud Drive, Dropbox, Google Drive). Files
            // there are silently replicated to the vendor's servers; a
            // history.jsonl containing literal credentials would
            // propagate. The startup-time string match catches the
            // common cases without I/O.
            if let Some(vendor) = secure_fs::is_likely_cloud_synced_path(&path) {
                eprintln!(
                    "dbboard: config path appears to be inside {vendor} ({}); \
                     query history may sync to the cloud. See README for how to \
                     exclude the dbboard config dir from {vendor} sync.",
                    path.display()
                );
            }
            let file = load_or_empty(&path)?;
            let admin = ConnectionAdmin::new_with_file(path, Arc::clone(&secrets), file.clone());
            // The same ConnectionAdmin instance is shared between the
            // Connections UI (which mutates it) and the DesktopSwitcher
            // (ADR-0020, which reads entries by id to look up the
            // backend config). Wrapping it in Arc<Mutex<_>> keeps both
            // sides looking at the same in-memory state.
            (file, Some(Arc::new(Mutex::new(admin))))
        }
        Err(_) => (ConnectionFile::empty(), None),
    };
    let backend = backend_config_from_env_and_store(&file, &*secrets)?;
    // Display label for the resolved connection (ADR-0017): stamped on
    // every `history.jsonl` completion record so a multi-connection
    // user can grep their log by target. Derived from the same env
    // snapshot + file pair `backend_config_from_env_and_store` used.
    let conn_label = resolved_connection_label(&file);

    // Open the query-history backing file (ADR-0017). Mirror of the
    // connection-store fallback above: a missing per-user config dir
    // degrades to an in-memory ring rather than aborting startup, and
    // a corrupt/unreadable file degrades to in-memory after logging.
    let history = match default_history_path() {
        Ok(path) => match PersistentHistoryStore::load_tail(path, DEFAULT_CAPACITY) {
            Ok(store) => {
                let skipped = store.skipped_on_load();
                if skipped > 0 {
                    eprintln!(
                        "dbboard: skipped {skipped} malformed history.jsonl line(s) at startup"
                    );
                }
                store
            }
            Err(e) => {
                eprintln!("dbboard: history.jsonl unreadable, falling back to in-memory only: {e}");
                PersistentHistoryStore::in_memory_only(DEFAULT_CAPACITY)
            }
        },
        Err(_) => PersistentHistoryStore::in_memory_only(DEFAULT_CAPACITY),
    };

    // The server runtime lives for the whole process. Connecting here
    // (before the window opens) preserves the fail-fast contract: a bad
    // connection string aborts startup instead of failing on first use.
    let rt = tokio::runtime::Runtime::new()?;
    let server = rt.block_on(serve(backend))?;
    let base_url = format!("http://127.0.0.1:{}", server.port);

    // The switcher needs four things in scope: the server's live
    // AppState (so swap_backend writes to the slot the router reads),
    // the shared ConnectionAdmin (to look up entries by id), the
    // SecretStore (for keyring lookups during backend_config_for_entry),
    // and a Handle to the server runtime (so build_adapter can drive
    // async I/O — the UI worker's current_thread runtime cannot host a
    // nested block_on). When the OS reports no per-user config dir we
    // fall through to a NullSwitcher that returns a Connection error,
    // matching the UI's "no admin available" affordance.
    let switcher: Arc<dyn ConnectionSwitcher> = match &admin {
        Some(admin) => Arc::new(DesktopSwitcher {
            state: server.state(),
            admin: Arc::clone(admin),
            secrets: Arc::clone(&secrets),
            rt: rt.handle().clone(),
        }),
        None => Arc::new(NullSwitcher),
    };

    // ADR-0023 + ADR-0025: AI provider bootstrap. The helper owns the
    // four moving parts (admin open, precedence chain, slot, switcher
    // selection) so `main()` stays at the wiring layer. Slice (b) also
    // returns the admin handle here so the Settings UI mutates the same
    // file the switcher reads from.
    let (ai_provider_slot, ai_switcher, ai_admin) = bootstrap_ai(&secrets);

    // ADR-0028 slice (c): hand the UI worker the same live-adapter
    // snapshot the HTTP router reads, so the `describe_table` fan-out
    // behind "Include column details" stays in-process and follows
    // connection switches automatically.
    let schema_source: Arc<dyn SchemaSource> = Arc::new(DesktopSchemaSource {
        state: server.state(),
    });

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([960.0, 640.0]),
        ..Default::default()
    };

    // ADR-0040: the startup update check runs on the same server runtime.
    // Clone a handle now — `rt` itself must stay in `main` because it
    // drives `server.shutdown()` after the window closes, so it cannot be
    // moved into the eframe closure below.
    let update_rt = rt.handle().clone();

    // ADR-0041: resolve the persisted colour theme before the window opens.
    // A missing config dir just means "no persistence" — the theme still
    // works for the session, it only cannot be remembered — so we keep the
    // default (Auto) and a `None` path rather than aborting. A malformed
    // file degrades to the default inside `load_ui_settings`.
    let ui_settings_path = default_ui_settings_path().ok();
    let theme = ui_settings_path
        .as_deref()
        .map_or_else(UiSettingsFile::default, load_ui_settings)
        .theme;

    let result = eframe::run_native(
        "dbboard",
        native_options,
        Box::new(move |cc| {
            install_cjk_font(&cc.egui_ctx);
            // ADR-0041: apply the persisted theme before the first paint so
            // there is no dark→light flash. `System` (our `Auto`) tracks the
            // OS setting live for the rest of the session.
            cc.egui_ctx.set_theme(egui_theme(theme));
            // Also push the theme to the OS window chrome so the Windows
            // title bar matches from the first frame (see `set_theme`).
            cc.egui_ctx
                .send_viewport_cmd(egui::ViewportCommand::SetTheme(viewport_theme(theme)));
            // Fire the best-effort update check as the window opens. It is
            // fully non-blocking: the state starts Idle/Checking and the
            // task flips it (and requests a repaint) when the GET lands.
            let update = update_check::spawn(&update_rt, cc.egui_ctx.clone());
            let inner = attach_annotations(DbboardApp::connect(
                base_url,
                cc.egui_ctx.clone(),
                history,
                conn_label,
                now_rfc3339,
                switcher,
                ai_switcher,
                ai_provider_slot,
                Some(schema_source),
            ));
            Ok(Box::new(DesktopApp::new(
                inner,
                admin,
                ai_admin,
                update,
                theme,
                ui_settings_path,
            )))
        }),
    );

    // The UI has exited; stop the server before reporting how it went.
    rt.block_on(server.shutdown())?;
    result.map_err(Into::into)
}

/// Top-level eframe app that owns the inner `DbboardApp` plus the
/// connection management window (ADR-0016). The wrapper exists because
/// `DbboardApp` is intentionally connection-agnostic — it talks to the
/// loopback server, not to the user's connection store — while the
/// Connections window mutates that store directly through a
/// `ConnectionAdmin`.
struct DesktopApp {
    inner: DbboardApp,
    connections: ConnectionsView,
    /// `None` when the OS reported no per-user config dir. The menu
    /// button is suppressed in that case rather than showing a window
    /// that would always fail to save. Wrapped in `Arc<Mutex<_>>` so
    /// the [`DesktopSwitcher`] (ADR-0020) can read entries by id from
    /// the same instance the UI mutates.
    admin: Option<Arc<Mutex<ConnectionAdmin>>>,
    /// AI provider Settings window (ADR-0025 slice b). Always present
    /// so the toggle state survives across `is_open` cycles; the menu
    /// button is suppressed when [`Self::ai_admin`] is `None`.
    ai_settings: AiSettingsView,
    /// Shared handle to `ai-providers.toml`. `None` when the OS
    /// reported no per-user config dir or the TOML was unreadable; the
    /// `AI Providers` menu button is hidden in that case rather than
    /// showing a window that could never save. Shared with the
    /// [`DesktopAiSwitcher`] so a swap performed by either side is
    /// visible to the other on the next frame.
    ai_admin: Option<Arc<Mutex<AiSettingsAdmin>>>,
    /// Id of a "Connect" click that has been dispatched but whose
    /// `ConnectionSwitched` / `SwitchFailed` reply has not yet landed
    /// (ADR-0020). Drives the Connections window auto-close: the window
    /// stays open while this is `Some` and closes the frame the switch
    /// succeeds. Users read a lingering window as "still connecting" and
    /// wait, so closing it is the clear "done" signal; a failure keeps
    /// it open so the error stays visible.
    pending_switch: Option<String>,
    /// Shared outcome of the startup update check (ADR-0040). Written once
    /// by the background task; read every frame the Help menu is open. The
    /// menu shows a notice only when this reaches
    /// [`update_check::UpdateState::Available`] — every other state (and a
    /// disabled or failed check) stays silent.
    update: update_check::SharedUpdateState,
    /// Current colour-theme choice (ADR-0041). Drives the ✓ in the Theme
    /// menu and is written to [`Self::ui_settings_path`] on every change.
    theme: ThemePreference,
    /// Where to persist [`Self::theme`]. `None` when the OS reported no
    /// per-user config dir — the Theme menu still works for the session,
    /// the choice just is not remembered across restarts.
    ui_settings_path: Option<PathBuf>,
    /// Parsed-Markdown cache for the update notice's release notes
    /// (ADR-0043). `egui_commonmark` re-parses `&str` every frame otherwise;
    /// the cache lives on the app so an open Help menu stays cheap. Empty
    /// and untouched until a newer release is found.
    commonmark_cache: egui_commonmark::CommonMarkCache,
}

impl DesktopApp {
    fn new(
        inner: DbboardApp,
        admin: Option<Arc<Mutex<ConnectionAdmin>>>,
        ai_admin: Option<Arc<Mutex<AiSettingsAdmin>>>,
        update: update_check::SharedUpdateState,
        theme: ThemePreference,
        ui_settings_path: Option<PathBuf>,
    ) -> Self {
        Self {
            inner,
            connections: ConnectionsView::new(),
            admin,
            ai_settings: AiSettingsView::new(),
            ai_admin,
            pending_switch: None,
            update,
            theme,
            ui_settings_path,
            commonmark_cache: egui_commonmark::CommonMarkCache::default(),
        }
    }

    /// Render the Theme submenu and apply a pick immediately (ADR-0041).
    /// Selecting a theme retints the running UI this frame via
    /// [`egui::Context::set_theme`] and best-effort-persists the choice;
    /// a persistence failure is logged, never surfaced, because the theme
    /// already applied and the app must not block on a settings write.
    fn theme_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button(t!("theme-menu"), |ui| {
            // Fixed order (Auto default first) so the submenu never
            // reshuffles as the active choice changes. Labels are looked
            // up with literal keys because `t!` only accepts literals.
            for (pref, label) in [
                (ThemePreference::Auto, t!("theme-auto")),
                (ThemePreference::Light, t!("theme-light")),
                (ThemePreference::Dark, t!("theme-dark")),
            ] {
                let active = self.theme == pref;
                let prefix = if active { "✓ " } else { "    " };
                if ui.button(format!("{prefix}{label}")).clicked() {
                    self.set_theme(ui.ctx(), pref);
                    ui.close();
                }
            }
        });
    }

    /// Adopt `pref` as the active theme: retint egui now and persist it.
    /// Persisting is best-effort and only attempted when a config path is
    /// known and the value actually changed (avoids a redundant write when
    /// the user re-picks the current theme).
    fn set_theme(&mut self, ctx: &egui::Context, pref: ThemePreference) {
        ctx.set_theme(egui_theme(pref));
        // egui retints its own painting, but the OS window chrome (the
        // Windows title bar) is drawn by winit and only follows the
        // *system* theme unless we push an explicit override. Sync it here
        // so a Dark pick doesn't leave a light title bar above a dark app.
        ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(viewport_theme(pref)));
        if self.theme == pref {
            return;
        }
        self.theme = pref;
        if let Some(path) = &self.ui_settings_path {
            if let Err(e) = save_ui_settings(path, &UiSettingsFile::with_theme(pref)) {
                eprintln!("dbboard: could not persist theme preference: {e}");
            }
        }
    }
}

/// Outcome of polling a dispatched-but-unresolved connection switch
/// (ADR-0020) so the Connections window can auto-close on success.
/// A free function, not a method, so it is unit-testable without an
/// egui context or a live worker.
#[derive(Debug, PartialEq, Eq)]
enum PendingSwitchPoll {
    /// Reply not in yet — keep the window open and keep polling.
    Waiting,
    /// The active connection now matches the request — close the window.
    Succeeded,
    /// The switch failed — keep the window open so the error is visible.
    Failed,
}

/// Decide what to do with a pending "Connect" click given the current
/// active-connection id and the last switch-error message.
///
/// Success is authoritative: the active id flipping to the requested one
/// means the worker rebuilt and swapped the adapter. A non-empty error is
/// only trusted once success has been ruled out, and [`switch_connection`]
/// clears the prior error at dispatch time so a stale failure can't be
/// read as this switch failing.
fn poll_pending_switch(
    pending: &str,
    active_id: &str,
    switch_error: Option<&str>,
) -> PendingSwitchPoll {
    if active_id == pending {
        PendingSwitchPoll::Succeeded
    } else if switch_error.is_some() {
        PendingSwitchPoll::Failed
    } else {
        PendingSwitchPoll::Waiting
    }
}

/// Map our persisted [`ThemePreference`] onto egui's runtime theme knob
/// (ADR-0041). `Auto` becomes `System`, which egui follows against the OS
/// light/dark setting for the life of the process. A free function so the
/// mapping is unit-testable without an egui context.
fn egui_theme(pref: ThemePreference) -> egui::ThemePreference {
    match pref {
        ThemePreference::Light => egui::ThemePreference::Light,
        ThemePreference::Dark => egui::ThemePreference::Dark,
        ThemePreference::Auto => egui::ThemePreference::System,
    }
}

/// Map our persisted [`ThemePreference`] onto the OS-chrome theme override
/// sent via [`egui::ViewportCommand::SetTheme`]. `Auto` becomes
/// `SystemDefault`, which clears the override so the title bar tracks the
/// OS setting again. A free function so the mapping is unit-testable
/// without a viewport.
fn viewport_theme(pref: ThemePreference) -> egui::SystemTheme {
    match pref {
        ThemePreference::Light => egui::SystemTheme::Light,
        ThemePreference::Dark => egui::SystemTheme::Dark,
        ThemePreference::Auto => egui::SystemTheme::SystemDefault,
    }
}

impl eframe::App for DesktopApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        egui::Panel::top("dbboard-menu").show_inside(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                if self.admin.is_some() && ui.button(t!("connections-window-title")).clicked() {
                    self.connections.open();
                }
                // ADR-0023 Decision 11: the AI menu entry is hidden
                // entirely when no provider was wired at startup
                // (graceful degradation = absence). When present, the
                // button toggles the panel open/closed; the open state
                // lives on `DbboardApp::ai_panel`.
                if self.inner.has_ai_provider() && ui.button(t!("ai-menu-button")).clicked() {
                    self.inner.toggle_ai_panel();
                }
                // ADR-0025 slice (b): the AI Settings menu is shown
                // whenever an `ai-providers.toml` admin is available,
                // independent of whether a provider is currently bound
                // — the whole point of the window is to bind one when
                // none is yet active.
                if self.ai_admin.is_some() && ui.button(t!("ai-settings-menu-button")).clicked() {
                    self.ai_settings.open();
                }
                language_menu(ui);
                self.theme_menu(ui);
                help_menu(ui, &self.update, &mut self.commonmark_cache);
            });
        });
        if let Some(admin) = &self.admin {
            // Same poison-handling rationale as the server's AppState
            // (ADR-0020): a panicked writer leaves the inner state valid,
            // so unwrap the poison and keep going rather than aborting.
            let mut guard = admin.lock().unwrap_or_else(PoisonError::into_inner);
            // Owned first: switch_error_message() borrows self.inner, but
            // the ui() call below needs a &mut borrow of self.connections,
            // so materialize the message before handing it over.
            let switch_error = self.inner.switch_error_message();
            // Auto-close the Connections window once a dispatched Connect
            // click resolves (ADR-0020). Polled before the window renders
            // so a successful switch closes it this frame instead of
            // flashing it open once more.
            if let Some(pending) = self.pending_switch.take() {
                match poll_pending_switch(
                    &pending,
                    self.inner.active_connection_id(),
                    switch_error.as_deref(),
                ) {
                    PendingSwitchPoll::Succeeded => self.connections.close(),
                    PendingSwitchPoll::Failed => {}
                    PendingSwitchPoll::Waiting => self.pending_switch = Some(pending),
                }
            }
            self.connections.ui(
                ui.ctx(),
                &mut guard,
                self.inner.active_connection_id(),
                switch_error.as_deref(),
            );
        }
        // ADR-0038: run any deferred export-save / import-pick native file
        // dialog now that the ConnectionAdmin lock (the `guard` above) is
        // released. The dialog blocks this thread for an unbounded time and
        // `DesktopSwitcher::switch` shares that lock, so it must never run
        // while the guard is held — mirrors the pending_connect drain.
        self.connections.drive_file_dialogs();
        // ADR-0025 slice (b): render the AI Settings window and push the
        // currently-active provider's display name down to the panel.
        // The push happens every frame (cheap clone of a short String)
        // because the active id can change without going through this
        // closure — e.g. a future CLI tool, or a rollback in the
        // switcher — and we want the subtitle to track reality.
        if let Some(ai_admin) = &self.ai_admin {
            let mut guard = ai_admin.lock().unwrap_or_else(PoisonError::into_inner);
            let active_id = guard.active_id().map(str::to_owned);
            let label = active_id.as_ref().and_then(|id| {
                guard
                    .entries()
                    .iter()
                    .find(|e| &e.id == id)
                    .map(|e| e.name.clone())
            });
            self.inner.set_active_ai_provider_label(label);
            self.ai_settings
                .ui(ui.ctx(), &mut guard, active_id.as_deref());
        } else {
            self.inner.set_active_ai_provider_label(None);
        }
        // ADR-0020: drain a "Connect" click from the Connections window
        // and turn it into a SwitchConnection command. Done before the
        // inner UI renders so the active-id marker on the next frame
        // already reflects the request (if it succeeds).
        if let Some(id) = self.connections.take_pending_connect() {
            // Remember the target so the next frames can poll for the
            // switch result and auto-close the window on success.
            self.pending_switch = Some(id.clone());
            self.inner.switch_connection(id);
        }
        // ADR-0025 slice (b): drain a "Use" click from the Settings
        // window and route it through the worker channel. The switch
        // happens off-thread; success or failure lands on
        // `DbboardApp::last_ai_switch_error` on the next `drain_replies`
        // pass.
        if let Some(id) = self.ai_settings.take_pending_switch() {
            self.inner.switch_ai_provider(id);
        }
        self.inner.ui(ui, frame);
    }
}

/// Production [`ConnectionSwitcher`] impl (ADR-0020). The worker thread
/// calls [`Self::switch`] when a `Command::SwitchConnection { id }`
/// arrives; this resolves the id against the shared connection store,
/// builds a fresh adapter on the server runtime, and atomically swaps
/// it into the live `AppState`. The HTTP contract is unchanged — only
/// the in-process wiring moves.
struct DesktopSwitcher {
    state: AppState,
    admin: Arc<Mutex<ConnectionAdmin>>,
    secrets: Arc<dyn SecretStore>,
    rt: tokio::runtime::Handle,
}

impl ConnectionSwitcher for DesktopSwitcher {
    fn switch(&self, id: &str) -> Result<(), DbError> {
        // Resolve the config under the lock, but drop the guard before
        // the (potentially slow) build_adapter await so concurrent UI
        // edits to the connection store are not blocked behind a TCP
        // handshake.
        let config = {
            let admin = self.admin.lock().unwrap_or_else(PoisonError::into_inner);
            let entry = admin
                .entries()
                .iter()
                .find(|e| e.id == id)
                .cloned()
                .ok_or_else(|| DbError::Connection(format!("unknown connection id: {id}")))?;
            backend_config_for_entry(&entry, &*self.secrets)
                .map_err(|e| DbError::Connection(e.to_string()))?
        };
        let adapter = build_adapter_on(&self.rt, config)?;
        swap_backend(&self.state, adapter);
        Ok(())
    }
}

/// Build an adapter on the server runtime from a thread that is *itself*
/// already inside a Tokio runtime.
///
/// [`DesktopSwitcher::switch`] runs on the worker's `current_thread`
/// runtime: it is invoked from `run_command_loop`, which the worker
/// drives with its own `block_on`. Calling `self.rt.block_on(..)` from
/// there panics with "Cannot block the current thread from within a
/// runtime" and silently kills the command-loop thread, after which
/// every later `Connect` click is a no-op — the "unresponsive Connect"
/// bug. It only surfaced once a connection's secret actually resolved,
/// because before that `backend_config_for_entry` returned `Err` *ahead*
/// of the `block_on`, so the switch failed cleanly instead of reaching
/// the panic.
///
/// The fix keeps the switch inline/blocking but crosses runtimes safely:
/// `spawn` the build onto the multi-thread server runtime (which owns
/// worker threads to drive it) and park this thread on a plain channel
/// until it completes. Parking is not a runtime operation, so it never
/// panics; the separate runtime makes progress independently, so there
/// is no deadlock.
fn build_adapter_on(
    rt: &tokio::runtime::Handle,
    config: BackendConfig,
) -> Result<Arc<dyn DatabaseAdapter>, DbError> {
    let (tx, rx) = std::sync::mpsc::channel();
    rt.spawn(async move {
        // The receiver is gone only if the worker is tearing down; then
        // dropping the built adapter here is the correct outcome.
        let _ = tx.send(build_adapter(config).await);
    });
    match rx.recv() {
        Ok(Ok(adapter)) => Ok(adapter),
        Ok(Err(ServerError::Backend(db))) => Err(db),
        Ok(Err(other)) => Err(DbError::Connection(other.to_string())),
        // The runtime dropped the task before it answered (e.g. the app
        // is shutting down). Surface a connection error rather than hang.
        Err(_) => Err(DbError::Connection(
            "adapter build task was cancelled".to_string(),
        )),
    }
}

/// Production [`SchemaSource`] impl (ADR-0028 slice (c)). One-method
/// pass-through to the server's `AppState`: the worker's
/// `PrefetchSchema` fan-out snapshots the same adapter the HTTP
/// handlers capture per request, so a connection switch between
/// commands is picked up on the next prefetch without extra plumbing.
struct DesktopSchemaSource {
    state: AppState,
}

impl SchemaSource for DesktopSchemaSource {
    fn current_adapter(&self) -> Arc<dyn DatabaseAdapter> {
        self.state.current_adapter()
    }
}

/// Fallback [`ConnectionSwitcher`] used when the OS reports no per-user
/// config dir (`admin` is `None`). The worker still has a switcher to
/// call, but every attempt surfaces a typed `Connection` error so the
/// UI shows "could not switch" instead of hanging.
struct NullSwitcher;

impl ConnectionSwitcher for NullSwitcher {
    fn switch(&self, _id: &str) -> Result<(), DbError> {
        Err(DbError::Connection(
            "no connection store available on this host; configure one to switch connections"
                .into(),
        ))
    }
}

/// Fallback [`AiProviderSwitcher`] (ADR-0025). Wired when the OS
/// reports no per-user config dir (`ai_admin` is `None`) — every
/// `SwitchAiProvider` command surfaces an
/// `AiError::Configuration("no ai store available")` so the UI shows a
/// usable error rather than hanging.
struct NullAiSwitcher;

impl AiProviderSwitcher for NullAiSwitcher {
    fn switch(&self, _id: &str) -> Result<(), AiError> {
        Err(AiError::Configuration("no ai store available".into()))
    }
}

/// Production [`AiProviderSwitcher`] (ADR-0025). The worker thread calls
/// [`Self::switch`] when a `Command::SwitchAiProvider { id }` arrives;
/// this resolves the id against the shared `ai-providers.toml` admin,
/// builds a fresh provider from the keyring secret, atomically swaps it
/// into the shared [`AiProviderSlot`] the worker reads from, and only
/// then persists the new active id to TOML. The HTTP contract is
/// unchanged — every AI call stays in-process (ADR-0023 Decision 3).
struct DesktopAiSwitcher {
    /// Shared with the Settings UI (slice b of issue 0008) so a swap
    /// performed by either side is visible to the other.
    admin: Arc<Mutex<AiSettingsAdmin>>,
    /// Same handle the connection store uses; lookups stay consistent
    /// with the UI's own keyring writes.
    secrets: Arc<dyn SecretStore>,
    /// The slot the worker reads on every AI command dispatch. Holding
    /// an `Arc` clone here lets us swap it without coordinating with
    /// the worker thread.
    slot: AiProviderSlot,
}

impl AiProviderSwitcher for DesktopAiSwitcher {
    fn switch(&self, id: &str) -> Result<(), AiError> {
        // Resolve the entry under the lock, but drop the guard before
        // the (potentially slow) provider construction so a concurrent
        // Settings UI edit is not blocked behind it. The `.kind` clone
        // is cheap (a String + an Option<String>).
        let kind: AiProviderKind = {
            let admin = self.admin.lock().unwrap_or_else(PoisonError::into_inner);
            admin
                .entries()
                .iter()
                .find(|e| e.id == id)
                .map(|entry| entry.kind.clone())
                .ok_or_else(|| AiError::Configuration(format!("unknown ai provider id: {id}")))?
        };
        let provider: Arc<dyn AiProvider> = build_provider_for_kind(&kind, &*self.secrets)?;

        // Swap the live slot *before* touching the TOML: a provider
        // that constructs successfully but fails to persist is still
        // better than a TOML that records an active id we never wired.
        {
            let mut guard = self.slot.write().unwrap_or_else(PoisonError::into_inner);
            *guard = Some(provider);
        }

        // Persist the new active id. A failure here means the running
        // process is correct (the slot points at the new provider) but
        // the next startup will pick whatever active id was on disk;
        // log loudly and proceed rather than rolling back, because the
        // user just saw the switch take effect in the panel.
        let mut admin = self.admin.lock().unwrap_or_else(PoisonError::into_inner);
        if let Err(e) = admin.set_active(Some(id.to_string())) {
            eprintln!(
                "dbboard: ai provider swapped to '{id}' in memory, but persisting active_id \
                 failed; next startup may pick a different provider: {e}"
            );
        }
        Ok(())
    }
}

/// Build an [`AiProvider`] from a [`AiProviderKind`] entry by looking
/// up the keyring secret it references. Shared between the startup
/// precedence chain and [`DesktopAiSwitcher::switch`] so both paths
/// agree on how each provider kind is constructed.
fn build_provider_for_kind(
    kind: &AiProviderKind,
    secrets: &dyn SecretStore,
) -> Result<Arc<dyn AiProvider>, AiError> {
    match kind {
        AiProviderKind::Anthropic {
            model,
            keyring_api_key_ref,
        } => {
            let key = secrets.get(keyring_api_key_ref).map_err(|e| {
                AiError::Configuration(format!(
                    "api key lookup failed for {keyring_api_key_ref}: {e}"
                ))
            })?;
            let provider = match model.as_deref().filter(|m| !m.trim().is_empty()) {
                Some(m) => AnthropicProvider::new(key, m)?,
                None => AnthropicProvider::with_default_model(key)?,
            };
            Ok(Arc::new(provider))
        }
    }
}

/// Stand up the optional AI layer (ADR-0023 + ADR-0025):
///
/// 1. Open `ai-providers.toml` when the OS reports a per-user config
///    dir; a corrupt/unreadable file is logged and degrades to no
///    admin (AI is opt-in, the env-var fallback still works).
/// 2. Run the precedence chain env > TOML > none to seed the initial
///    provider, wrap it in the shared [`AiProviderSlot`] the worker
///    will read from.
/// 3. Pick the matching [`AiProviderSwitcher`]: the real
///    [`DesktopAiSwitcher`] when admin is present (so the future
///    Settings UI and the slot share the same handle), the
///    [`NullAiSwitcher`] otherwise.
///
/// Returns the slot, the switcher, and the admin handle. Slice (b)
/// (ADR-0025) wires the third element into [`DesktopApp`] so the
/// Settings UI mutates the same `ai-providers.toml` the
/// [`DesktopAiSwitcher`] reads from. `None` means the OS reported no
/// per-user config dir or the TOML was unreadable; the menu hides the
/// Settings entry in that case.
fn bootstrap_ai(
    secrets: &Arc<dyn SecretStore>,
) -> (
    AiProviderSlot,
    Arc<dyn AiProviderSwitcher>,
    Option<Arc<Mutex<AiSettingsAdmin>>>,
) {
    let ai_admin: Option<Arc<Mutex<AiSettingsAdmin>>> = match default_ai_providers_path() {
        Ok(path) => match AiSettingsAdmin::open(path, Arc::clone(secrets)) {
            Ok(admin) => Some(Arc::new(Mutex::new(admin))),
            Err(e) => {
                eprintln!(
                    "dbboard: ai-providers.toml unreadable, AI TOML store disabled (env var \
                     fallback still works): {e}"
                );
                None
            }
        },
        Err(_) => None,
    };

    let ai_provider: Option<Arc<dyn AiProvider>> = resolve_ai_provider_from(
        std::env::var("DBBOARD_ANTHROPIC_API_KEY").ok().as_deref(),
        std::env::var("DBBOARD_ANTHROPIC_MODEL").ok().as_deref(),
        ai_admin.as_deref(),
        &**secrets,
    );
    let slot: AiProviderSlot = Arc::new(RwLock::new(ai_provider));

    let switcher: Arc<dyn AiProviderSwitcher> = match &ai_admin {
        Some(admin) => Arc::new(DesktopAiSwitcher {
            admin: Arc::clone(admin),
            secrets: Arc::clone(secrets),
            slot: Arc::clone(&slot),
        }),
        None => Arc::new(NullAiSwitcher),
    };
    (slot, switcher, ai_admin)
}

/// Attach the local note store (ADR-0045) to a freshly-connected app.
/// Kept as a wrapper so `main` stays a single expression; a missing or
/// unreadable file degrades to no notes via [`open_annotations`].
fn attach_annotations(app: DbboardApp) -> DbboardApp {
    match open_annotations() {
        Some(admin) => app.with_annotations(admin),
        None => app,
    }
}

/// Open the local table/column note store (`annotations.toml`, ADR-0045).
///
/// Returns `None` — the Structure tab's Note column stays read-only — when
/// the config dir can't be resolved or the file is unreadable/corrupt. The
/// notes carry no secret and never touch a database, so a load failure is
/// logged and degrades gracefully rather than aborting startup.
fn open_annotations() -> Option<AnnotationsAdmin> {
    let path = default_annotations_path().ok()?;
    match AnnotationsAdmin::new_with_file(path) {
        Ok(admin) => Some(admin),
        Err(e) => {
            eprintln!("dbboard: annotations.toml unreadable, local notes disabled: {e}");
            None
        }
    }
}

/// Resolve the optional AI provider via the precedence chain
/// **env > `ai-providers.toml` active id > none** (ADR-0023 + ADR-0025).
///
/// The env path (`DBBOARD_ANTHROPIC_API_KEY` + optional
/// `DBBOARD_ANTHROPIC_MODEL`) preserves Stage 1 back-compat: a user who
/// already exports the key keeps working even if `ai-providers.toml` is
/// absent or has a different active id. A user who has only TOML
/// configured falls through to the second branch. Every failure on
/// either branch (missing key, empty trim, keyring miss, construction
/// error) logs to stderr and degrades to `None` — AI is opt-in and a
/// misconfigured optional layer must never brick startup.
///
/// Tests inject `env_*` directly to avoid touching real process env
/// (which would race other tests). The binary reads from `std::env::var`
/// at the call site.
fn resolve_ai_provider_from(
    env_api_key: Option<&str>,
    env_model: Option<&str>,
    ai_admin: Option<&Mutex<AiSettingsAdmin>>,
    secrets: &dyn SecretStore,
) -> Option<Arc<dyn AiProvider>> {
    if let Some(key) = env_api_key.map(str::trim).filter(|k| !k.is_empty()) {
        let model = env_model.map(str::trim).filter(|m| !m.is_empty());
        let result = match model {
            Some(m) => AnthropicProvider::new(key, m),
            None => AnthropicProvider::with_default_model(key),
        };
        return match result {
            Ok(provider) => Some(Arc::new(provider)),
            Err(e) => {
                eprintln!("dbboard: AI provider init from env failed, AI panel disabled: {e}");
                None
            }
        };
    }

    let admin = ai_admin?;
    let (active_id, kind) = {
        let guard = admin.lock().unwrap_or_else(PoisonError::into_inner);
        let id = guard.active_id()?.to_string();
        let kind = guard
            .entries()
            .iter()
            .find(|e| e.id == id)
            .map(|entry| entry.kind.clone());
        (id, kind)
    };
    let Some(kind) = kind else {
        eprintln!(
            "dbboard: ai-providers.toml active_id='{active_id}' refers to no entry; AI panel \
             disabled"
        );
        return None;
    };
    match build_provider_for_kind(&kind, secrets) {
        Ok(provider) => Some(provider),
        Err(e) => {
            eprintln!(
                "dbboard: AI provider init from ai-providers.toml (active_id='{active_id}') \
                 failed, AI panel disabled: {e}"
            );
            None
        }
    }
}

/// Wall-clock RFC 3339 formatter for ADR-0017 `history.jsonl` records.
/// Injected into `DbboardApp` so `dbboard-ui` itself stays free of any
/// date-formatting crate. A formatting failure (effectively impossible
/// with the RFC 3339 description) degrades to the empty string rather
/// than panicking the UI thread — the record is still appended, just
/// without a parseable `ts`.
fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}

/// Render the runtime locale switcher submenu inside the menu bar
/// (ADR-0022). One row per [`SUPPORTED_LOCALES`] entry: the active
/// locale is prefixed with `✓`, the rest with two non-breaking spaces
/// so every label lines up. Clicking a row swaps the global Fluent
/// loader in place via [`dbboard_i18n::set_language`] and asks egui
/// for a repaint so the next frame redraws every string against the
/// new bundle.
///
/// Swap failures (e.g. a malformed `.ftl` in a freshly added locale)
/// are logged but non-fatal — the previous locale stays selected so
/// the UI keeps painting.
/// Product + version line shown at the top of the Help menu.
///
/// Built from the binary crate's own `CARGO_PKG_VERSION` so a handoff
/// bug report from a (non-technical) collector user can be pinned to an
/// exact build. Deliberately not translated — it is a product name plus
/// a semantic version, identical in every locale.
fn about_line() -> String {
    format!("dbboard {}", env!("CARGO_PKG_VERSION"))
}

/// Canonical public home of the project: latest builds, docs, and the
/// place a handoff bug report should go. Surfaced as a clickable row in
/// the Help menu so a collector user can always find their way back to
/// the source. Not translated — it is a bare URL, identical everywhere.
const REPO_URL: &str = "https://github.com/meta-taro/dbboard";

/// Help menu (internal distribution). Read-only rows: the running
/// version (`about_line`), a one-line pointer at the setup docs, and a
/// clickable link to the public project repo (`REPO_URL`). Kept
/// intentionally tiny — the collector users this ships to need "what
/// version am I on", "where do I look", and "where does this come from"
/// far more than a rich About window.
/// Close behavior for the Help menu. egui menus default to
/// [`egui::PopupCloseBehavior::CloseOnClick`], which dismisses the whole
/// menu on *any* click — inside or outside. The Help menu now carries
/// interactive content (the update hyperlink and a collapsible changelog
/// from `render_update_notice`), and that default swallows the first click
/// on a link or the "release notes" toggle, slamming the menu shut before
/// the widget can react. `CloseOnClickOutside` keeps the menu open while
/// the user interacts with its contents and only dismisses it on a click
/// outside the popup body.
fn help_menu_close_behavior() -> egui::PopupCloseBehavior {
    egui::PopupCloseBehavior::CloseOnClickOutside
}

fn help_menu(
    ui: &mut egui::Ui,
    update: &update_check::SharedUpdateState,
    md_cache: &mut egui_commonmark::CommonMarkCache,
) {
    egui::containers::menu::MenuButton::new(t!("help-menu"))
        .config(
            egui::containers::menu::MenuConfig::new().close_behavior(help_menu_close_behavior()),
        )
        .ui(ui, |ui| {
            ui.label(about_line());
            render_update_notice(ui, update, md_cache);
            ui.separator();
            ui.label(t!("help-docs-hint"));
            ui.hyperlink_to(t!("help-repo-link"), REPO_URL);
        });
}

/// Render the update notice under the version line — but only when the
/// check found a newer release (ADR-0040). `Idle` / `Checking` /
/// `UpToDate` / `Failed` all render nothing: the feature informs, it never
/// nags, and a failed or offline check must be indistinguishable from "up
/// to date".
///
/// When shown, the notice names the new version, links to its release
/// page, and offers the release notes as a collapsible, selectable
/// (copyable) changelog. Updating stays fully manual — there is no
/// download button here on purpose.
fn render_update_notice(
    ui: &mut egui::Ui,
    update: &update_check::SharedUpdateState,
    md_cache: &mut egui_commonmark::CommonMarkCache,
) {
    let snapshot = update
        .lock()
        .map_or(update_check::UpdateState::Idle, |guard| guard.clone());

    let update_check::UpdateState::Available(info) = snapshot else {
        return;
    };

    ui.separator();
    ui.label(
        egui::RichText::new(t_args!(
            "help-update-available",
            version = info.version.clone()
        ))
        .strong(),
    );
    if !info.url.is_empty() {
        ui.hyperlink_to(t!("help-update-link"), &info.url);
    }
    if !info.notes.is_empty() {
        ui.collapsing(t!("help-update-notes"), |ui| {
            egui::ScrollArea::vertical()
                .max_height(200.0)
                .show(ui, |ui| {
                    // The release body arrives as CommonMark; render it so
                    // headings/bold/code/links read as formatted text
                    // instead of literal `**source**` (ADR-0043). The viewer
                    // keeps text selectable, preserving the Ctrl+C-into-a-
                    // report affordance of the old plain label (ADR-0039).
                    egui_commonmark::CommonMarkViewer::new().show(ui, md_cache, &info.notes);
                });
        });
    }
}

fn language_menu(ui: &mut egui::Ui) {
    ui.menu_button(t!("language-menu"), |ui| {
        let current = dbboard_i18n::current_language().to_string();
        for (tag, native) in SUPPORTED_LOCALES {
            // BCP-47 region subtags are upper-case by convention but
            // both sides come from string literals; case-insensitive
            // compare is the defensive choice if `unic-langid` ever
            // normalises differently.
            let active = current.eq_ignore_ascii_case(tag);
            let prefix = if active { "✓ " } else { "    " };
            if ui.button(format!("{prefix}{native}")).clicked() {
                if let Err(e) = dbboard_i18n::set_language(tag) {
                    eprintln!("dbboard: locale switch to {tag} failed: {e}");
                }
                ui.ctx().request_repaint();
                ui.close();
            }
        }
    });
}

/// Look up an OS-installed CJK font and append it to egui's font stack
/// (ADR-0015). egui's bundled `Ubuntu-Light` covers Latin + Cyrillic
/// but renders CJK as tofu; appending a CJK font as a *fallback* (not a
/// replacement) keeps the existing look for Latin while resolving the
/// CJK ranges from the system.
///
/// We probe one path per family and stop at the first hit. A miss is
/// logged but non-fatal — bundling Noto CJK ourselves is a deferred
/// Stage 2 decision (~20 MB per script).
fn install_cjk_font(ctx: &egui::Context) {
    let Some((name, bytes)) = load_first_cjk_font() else {
        eprintln!(
            "dbboard: no CJK system font found; ja/ko/zh users may see \
             tofu. Install Noto Sans CJK (Linux) or Yu Gothic / PingFang \
             / Hiragino (Windows/macOS) to fix."
        );
        return;
    };

    let mut fonts = egui::FontDefinitions::default();
    fonts
        .font_data
        .insert(name.to_owned(), egui::FontData::from_owned(bytes).into());
    // Append, do not replace. Egui walks the family in order; Latin
    // glyphs keep coming from Ubuntu-Light, CJK glyphs fall through.
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push(name.to_owned());
    }
    ctx.set_fonts(fonts);
}

/// Probe a small per-OS candidate list. The first readable file wins —
/// we do not try to pick "the best" CJK font, only "any CJK font" so
/// the user does not see tofu.
fn load_first_cjk_font() -> Option<(&'static str, Vec<u8>)> {
    #[cfg(target_os = "windows")]
    const CANDIDATES: &[(&str, &str)] = &[
        ("YuGothic", r"C:\Windows\Fonts\YuGothM.ttc"),
        ("YuGothicUI", r"C:\Windows\Fonts\YuGothR.ttc"),
        ("Meiryo", r"C:\Windows\Fonts\meiryo.ttc"),
        ("MSGothic", r"C:\Windows\Fonts\msgothic.ttc"),
        ("MalgunGothic", r"C:\Windows\Fonts\malgun.ttf"),
        ("MicrosoftYaHei", r"C:\Windows\Fonts\msyh.ttc"),
    ];
    #[cfg(target_os = "macos")]
    const CANDIDATES: &[(&str, &str)] = &[
        ("HiraginoSans", "/System/Library/Fonts/Hiragino Sans GB.ttc"),
        ("PingFang", "/System/Library/Fonts/PingFang.ttc"),
        (
            "AppleSDGothicNeo",
            "/System/Library/Fonts/AppleSDGothicNeo.ttc",
        ),
    ];
    #[cfg(all(unix, not(target_os = "macos")))]
    const CANDIDATES: &[(&str, &str)] = &[
        (
            "NotoSansCJK",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        ),
        (
            "NotoSansCJK",
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        ),
        (
            "NotoSansCJKJP",
            "/usr/share/fonts/opentype/noto/NotoSansCJKjp-Regular.otf",
        ),
        (
            "NotoSansCJKKR",
            "/usr/share/fonts/opentype/noto/NotoSansCJKkr-Regular.otf",
        ),
    ];
    #[cfg(not(any(target_os = "windows", target_os = "macos", unix)))]
    const CANDIDATES: &[(&str, &str)] = &[];

    for (name, path) in CANDIDATES {
        if let Ok(bytes) = std::fs::read(path) {
            return Some((*name, bytes));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    //! Tests for the slice a-2-β AI wiring: the env > TOML > none
    //! precedence chain ([`resolve_ai_provider_from`]) and the in-process
    //! [`DesktopAiSwitcher`].
    //!
    //! Tests inject `env_*` directly rather than mutating
    //! `std::env::*` — Rust test binaries run in parallel by default,
    //! and a real env mutation would race other tests on the same
    //! process. `AnthropicProvider::new` / `with_default_model` are
    //! constructors that only validate the key locally (no network
    //! call), so a non-empty placeholder key is enough to land a real
    //! `Arc<dyn AiProvider>` in the slot.
    use super::{
        build_provider_for_kind, resolve_ai_provider_from, AiProviderSwitcher, DesktopAiSwitcher,
    };
    use dbboard_ai::{AiError, AiProvider};
    use dbboard_config::{
        AiProviderEntry, AiProviderFile, AiProviderKind, AiSettingsAdmin, InMemorySecretStore,
        SecretStore, AI_CONFIG_VERSION,
    };
    use dbboard_ui::AiProviderSlot;
    use std::sync::{Arc, Mutex, RwLock};
    use tempfile::TempDir;

    fn admin_with(
        entries: Vec<AiProviderEntry>,
        active_id: Option<&str>,
        secrets: Arc<dyn SecretStore>,
    ) -> (Arc<Mutex<AiSettingsAdmin>>, TempDir) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("ai-providers.toml");
        let file = AiProviderFile {
            version: AI_CONFIG_VERSION,
            active_id: active_id.map(str::to_string),
            providers: entries,
        };
        let admin = AiSettingsAdmin::new_with_file(path, secrets, file);
        (Arc::new(Mutex::new(admin)), tmp)
    }

    fn anthropic_entry(id: &str, keyring_ref: &str) -> AiProviderEntry {
        AiProviderEntry {
            id: id.to_string(),
            name: id.to_string(),
            kind: AiProviderKind::Anthropic {
                model: None,
                keyring_api_key_ref: keyring_ref.to_string(),
            },
        }
    }

    fn empty_slot() -> AiProviderSlot {
        Arc::new(RwLock::new(None))
    }

    #[test]
    fn theme_preference_maps_onto_egui_theme() {
        use super::egui_theme;
        use dbboard_config::ThemePreference;
        // Auto is the important case: it must become egui's `System` so the
        // running UI tracks the OS light/dark setting (ADR-0041), not a
        // frozen light or dark.
        assert_eq!(
            egui_theme(ThemePreference::Auto),
            egui::ThemePreference::System
        );
        assert_eq!(
            egui_theme(ThemePreference::Light),
            egui::ThemePreference::Light
        );
        assert_eq!(
            egui_theme(ThemePreference::Dark),
            egui::ThemePreference::Dark
        );
    }

    #[test]
    fn theme_preference_maps_onto_viewport_theme() {
        use super::viewport_theme;
        use dbboard_config::ThemePreference;
        // Auto must clear the OS-chrome override (SystemDefault) so the
        // title bar follows the OS again; explicit picks force the matching
        // title-bar theme so it can't diverge from the app body.
        assert_eq!(
            viewport_theme(ThemePreference::Auto),
            egui::SystemTheme::SystemDefault
        );
        assert_eq!(
            viewport_theme(ThemePreference::Light),
            egui::SystemTheme::Light
        );
        assert_eq!(
            viewport_theme(ThemePreference::Dark),
            egui::SystemTheme::Dark
        );
    }

    #[test]
    fn help_menu_stays_open_on_inside_clicks() {
        // Regression: the Help menu carries an update hyperlink and a
        // collapsible changelog. The egui default (`CloseOnClick`) closes
        // the menu on the first inside click, so the link and the notes
        // toggle were unusable. It must close only on an *outside* click.
        assert_eq!(
            super::help_menu_close_behavior(),
            egui::PopupCloseBehavior::CloseOnClickOutside
        );
    }

    #[test]
    fn env_wins_even_when_toml_active_id_would_fail() {
        // Admin has an entry whose keyring secret is NOT registered, so
        // taking the TOML branch would yield `None`. If env wins, the
        // result is `Some` despite the broken TOML entry.
        let secrets: Arc<dyn SecretStore> = Arc::new(InMemorySecretStore::default());
        let (admin, _tmp) = admin_with(
            vec![anthropic_entry("broken", "dbboard.ai.broken.api_key")],
            Some("broken"),
            Arc::clone(&secrets),
        );
        let resolved =
            resolve_ai_provider_from(Some("sk-env-test-key"), None, Some(&*admin), &*secrets);
        assert!(
            resolved.is_some(),
            "env path should produce a provider regardless of TOML state"
        );
    }

    #[test]
    fn toml_active_id_wins_when_env_is_blank() {
        let secrets: Arc<dyn SecretStore> = Arc::new(InMemorySecretStore::default());
        secrets
            .set("dbboard.ai.primary.api_key", "sk-toml-test-key")
            .expect("seed keyring");
        let (admin, _tmp) = admin_with(
            vec![anthropic_entry("primary", "dbboard.ai.primary.api_key")],
            Some("primary"),
            Arc::clone(&secrets),
        );
        let resolved = resolve_ai_provider_from(
            // Blank-trim is treated as "not set" so a shell exporting
            // `DBBOARD_ANTHROPIC_API_KEY=` does not silently disable AI.
            Some("   "),
            None,
            Some(&*admin),
            &*secrets,
        );
        assert!(
            resolved.is_some(),
            "TOML path should resolve when env is blank-trim"
        );
    }

    #[test]
    fn returns_none_when_admin_has_no_active_id() {
        let secrets: Arc<dyn SecretStore> = Arc::new(InMemorySecretStore::default());
        let (admin, _tmp) = admin_with(
            vec![anthropic_entry("p", "dbboard.ai.p.api_key")],
            None,
            Arc::clone(&secrets),
        );
        let resolved = resolve_ai_provider_from(None, None, Some(&*admin), &*secrets);
        assert!(resolved.is_none());
    }

    #[test]
    fn returns_none_when_no_env_and_no_admin() {
        let secrets: Arc<dyn SecretStore> = Arc::new(InMemorySecretStore::default());
        let resolved = resolve_ai_provider_from(None, None, None, &*secrets);
        assert!(resolved.is_none());
    }

    #[test]
    fn toml_path_returns_none_when_keyring_lookup_fails() {
        let secrets: Arc<dyn SecretStore> = Arc::new(InMemorySecretStore::default());
        // active_id points at an entry whose keyring secret was never
        // registered — graceful degradation to `None` rather than panic.
        let (admin, _tmp) = admin_with(
            vec![anthropic_entry("ghost", "dbboard.ai.ghost.api_key")],
            Some("ghost"),
            Arc::clone(&secrets),
        );
        let resolved = resolve_ai_provider_from(None, None, Some(&*admin), &*secrets);
        assert!(resolved.is_none());
    }

    #[test]
    fn build_provider_for_kind_uses_default_model_when_kind_has_none() {
        let secrets: Arc<dyn SecretStore> = Arc::new(InMemorySecretStore::default());
        secrets
            .set("dbboard.ai.k.api_key", "sk-x")
            .expect("seed keyring");
        let kind = AiProviderKind::Anthropic {
            model: None,
            keyring_api_key_ref: "dbboard.ai.k.api_key".into(),
        };
        let provider = build_provider_for_kind(&kind, &*secrets).expect("provider built");
        // Construction succeeded — observable through the Arc being
        // non-null and the AiProvider trait being object-safe.
        let _: Arc<dyn AiProvider> = provider;
    }

    #[test]
    fn build_provider_for_kind_propagates_keyring_miss_as_configuration_error() {
        let secrets: Arc<dyn SecretStore> = Arc::new(InMemorySecretStore::default());
        let kind = AiProviderKind::Anthropic {
            model: None,
            keyring_api_key_ref: "dbboard.ai.absent.api_key".into(),
        };
        match build_provider_for_kind(&kind, &*secrets) {
            Err(AiError::Configuration(msg)) => {
                assert!(
                    msg.contains("api key lookup failed"),
                    "expected configuration error mentioning lookup, got {msg}"
                );
            }
            Err(other) => panic!("expected Configuration error, got {other:?}"),
            Ok(_) => panic!("expected Configuration error, got Ok provider"),
        }
    }

    #[test]
    fn desktop_ai_switcher_swaps_slot_and_persists_active_id() {
        let secrets: Arc<dyn SecretStore> = Arc::new(InMemorySecretStore::default());
        secrets
            .set("dbboard.ai.primary.api_key", "sk-test")
            .expect("seed keyring");
        let (admin, _tmp) = admin_with(
            vec![anthropic_entry("primary", "dbboard.ai.primary.api_key")],
            None,
            Arc::clone(&secrets),
        );
        let slot = empty_slot();
        let switcher = DesktopAiSwitcher {
            admin: Arc::clone(&admin),
            secrets: Arc::clone(&secrets),
            slot: Arc::clone(&slot),
        };

        switcher.switch("primary").expect("switch ok");

        assert!(
            slot.read().unwrap().is_some(),
            "slot should be populated after a successful switch"
        );
        assert_eq!(
            admin
                .lock()
                .unwrap()
                .active_id()
                .map(str::to_string)
                .as_deref(),
            Some("primary"),
            "active_id should persist alongside the in-memory swap"
        );
    }

    #[test]
    fn desktop_ai_switcher_rejects_unknown_id_and_leaves_slot_untouched() {
        let secrets: Arc<dyn SecretStore> = Arc::new(InMemorySecretStore::default());
        let (admin, _tmp) = admin_with(Vec::new(), None, Arc::clone(&secrets));
        let slot = empty_slot();
        let switcher = DesktopAiSwitcher {
            admin: Arc::clone(&admin),
            secrets: Arc::clone(&secrets),
            slot: Arc::clone(&slot),
        };

        let err = switcher
            .switch("nope")
            .expect_err("unknown id should error");
        match err {
            AiError::Configuration(msg) => {
                assert!(
                    msg.contains("unknown ai provider id"),
                    "expected 'unknown ai provider id', got {msg}"
                );
            }
            other => panic!("expected Configuration error, got {other:?}"),
        }
        assert!(
            slot.read().unwrap().is_none(),
            "slot must stay empty when switch fails"
        );
        assert!(
            admin.lock().unwrap().active_id().is_none(),
            "active_id must stay None when switch fails"
        );
    }

    #[test]
    fn desktop_ai_switcher_leaves_slot_untouched_when_keyring_lookup_fails() {
        let secrets: Arc<dyn SecretStore> = Arc::new(InMemorySecretStore::default());
        let (admin, _tmp) = admin_with(
            vec![anthropic_entry("ghost", "dbboard.ai.ghost.api_key")],
            None,
            Arc::clone(&secrets),
        );
        let slot = empty_slot();
        let switcher = DesktopAiSwitcher {
            admin: Arc::clone(&admin),
            secrets: Arc::clone(&secrets),
            slot: Arc::clone(&slot),
        };

        let err = switcher
            .switch("ghost")
            .expect_err("keyring miss should error");
        assert!(matches!(err, AiError::Configuration(_)));
        assert!(
            slot.read().unwrap().is_none(),
            "slot must stay empty when the keyring lookup fails"
        );
        assert!(
            admin.lock().unwrap().active_id().is_none(),
            "active_id must stay None when the keyring lookup fails"
        );
    }

    #[test]
    fn build_adapter_on_does_not_panic_inside_the_worker_runtime() {
        use super::{build_adapter_on, BackendConfig};

        // Reproduce the exact thread topology that broke Connect: a
        // multi-thread *server* runtime (as `main` builds via
        // `Runtime::new`) whose `Handle` lives in `DesktopSwitcher`, and
        // the worker's `current_thread` runtime that drives the command
        // loop. `switch` runs inside the latter. Before the fix it called
        // `Handle::block_on` there, which panics with "Cannot block the
        // current thread from within a runtime" and killed the command
        // loop — after which every Connect click was a silent no-op.
        // `build_adapter_on` must complete without panicking from this
        // context; `:memory:` Turso is the standard offline test backend.
        let server = tokio::runtime::Runtime::new().expect("server runtime");
        let handle = server.handle().clone();
        let worker = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("worker runtime");

        let result =
            worker.block_on(async { build_adapter_on(&handle, BackendConfig::turso(":memory:")) });

        assert!(
            result.is_ok(),
            "switch must build the adapter without panicking inside the worker runtime; got err: {:?}",
            result.err()
        );
    }

    #[test]
    fn poll_pending_switch_waits_until_a_reply_lands() {
        use super::{poll_pending_switch, PendingSwitchPoll};
        // Just dispatched: active id still the old one, error cleared at
        // dispatch. The window must stay open (no premature close/fail).
        assert_eq!(
            poll_pending_switch("store-a", "", None),
            PendingSwitchPoll::Waiting
        );
        assert_eq!(
            poll_pending_switch("store-a", "prod-pg", None),
            PendingSwitchPoll::Waiting
        );
    }

    #[test]
    fn poll_pending_switch_closes_on_success() {
        use super::{poll_pending_switch, PendingSwitchPoll};
        // Active id flipped to the requested target: adapter swapped.
        assert_eq!(
            poll_pending_switch("store-a", "store-a", None),
            PendingSwitchPoll::Succeeded
        );
    }

    #[test]
    fn poll_pending_switch_keeps_window_open_on_failure() {
        use super::{poll_pending_switch, PendingSwitchPoll};
        // Error present and active id unchanged: the switch failed, so the
        // window stays open to show it.
        assert_eq!(
            poll_pending_switch("store-a", "prod-pg", Some("could not connect")),
            PendingSwitchPoll::Failed
        );
    }

    #[test]
    fn poll_pending_switch_prefers_success_over_a_lingering_error() {
        use super::{poll_pending_switch, PendingSwitchPoll};
        // Belt-and-braces: even if an error string is somehow still set,
        // an active id matching the request means the switch landed.
        assert_eq!(
            poll_pending_switch("store-a", "store-a", Some("stale")),
            PendingSwitchPoll::Succeeded
        );
    }

    #[test]
    fn about_line_carries_the_crate_version() {
        use super::about_line;
        // The Help > About line shown to (non-technical) collector users
        // must name the product and the exact running version, so a
        // handoff bug report can be pinned to a build. The version is the
        // binary crate's own `CARGO_PKG_VERSION`, not a hard-coded string.
        let line = about_line();
        assert!(line.starts_with("dbboard "), "got: {line}");
        assert!(
            line.contains(env!("CARGO_PKG_VERSION")),
            "about line must embed the crate version, got: {line}"
        );
    }

    #[test]
    fn repo_url_points_at_the_public_github_repo() {
        use super::REPO_URL;
        // The Help menu offers the collector user a way back to the
        // canonical source: latest builds, docs, and where to file a
        // handoff bug report. Must be the public https GitHub repo.
        assert_eq!(REPO_URL, "https://github.com/meta-taro/dbboard");
        assert!(REPO_URL.starts_with("https://"), "must be https");
    }
}
