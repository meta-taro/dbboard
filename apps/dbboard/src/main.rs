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
//! Locale resolution (ADR-0015) runs here too: `DBBOARD_LANG` > OS
//! locale > `en`. The binary also registers an OS CJK font into the
//! egui font stack so `ja` / `ko` / `zh-CN` / `zh-TW` users do not see
//! tofu — egui's bundled Ubuntu-Light covers Latin + Cyrillic but no
//! CJK ranges.

use std::sync::{Arc, Mutex, PoisonError};

use dbboard_config::store::{default_history_path, default_path, load_or_empty};
use dbboard_config::{ConnectionAdmin, ConnectionFile, KeyringStore, SecretStore};
use dbboard_i18n::t;
use dbboard_server::{
    backend_config_for_entry, backend_config_from_env_and_store, build_adapter,
    resolved_connection_label, serve, swap_backend, AppState, ServerError,
};
use dbboard_ui::{
    ConnectionSwitcher, ConnectionsView, DbError, DbboardApp, PersistentHistoryStore,
    DEFAULT_CAPACITY,
};
use time::format_description::well_known::Rfc3339;

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

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([960.0, 640.0]),
        ..Default::default()
    };

    let result = eframe::run_native(
        "dbboard",
        native_options,
        Box::new(move |cc| {
            install_cjk_font(&cc.egui_ctx);
            let inner = DbboardApp::connect(
                base_url,
                cc.egui_ctx.clone(),
                history,
                conn_label,
                now_rfc3339,
                switcher,
            );
            Ok(Box::new(DesktopApp::new(inner, admin)))
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
}

impl DesktopApp {
    fn new(inner: DbboardApp, admin: Option<Arc<Mutex<ConnectionAdmin>>>) -> Self {
        Self {
            inner,
            connections: ConnectionsView::new(),
            admin,
        }
    }
}

impl eframe::App for DesktopApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        egui::Panel::top("dbboard-menu").show_inside(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                if self.admin.is_some() && ui.button(t!("connections-window-title")).clicked() {
                    self.connections.open();
                }
                language_menu(ui);
            });
        });
        if let Some(admin) = &self.admin {
            // Same poison-handling rationale as the server's AppState
            // (ADR-0020): a panicked writer leaves the inner state valid,
            // so unwrap the poison and keep going rather than aborting.
            let mut guard = admin.lock().unwrap_or_else(PoisonError::into_inner);
            self.connections
                .ui(ui.ctx(), &mut guard, self.inner.active_connection_id());
        }
        // ADR-0020: drain a "Connect" click from the Connections window
        // and turn it into a SwitchConnection command. Done before the
        // inner UI renders so the active-id marker on the next frame
        // already reflects the request (if it succeeds).
        if let Some(id) = self.connections.take_pending_connect() {
            self.inner.switch_connection(id);
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
        let adapter = self
            .rt
            .block_on(build_adapter(config))
            .map_err(|e| match e {
                ServerError::Backend(db) => db,
                other => DbError::Connection(other.to_string()),
            })?;
        swap_backend(&self.state, adapter);
        Ok(())
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
