//! Internationalisation glue for `dbboard-ui` (ADR-0015).
//!
//! This crate owns three things:
//!
//! 1. The embedded `.ftl` resources under `crates/dbboard-i18n/i18n/`.
//!    They are walked at compile time by [`rust_embed`] and bound to
//!    the binary, so the default install needs no on-disk translation
//!    files.
//! 2. A global [`FluentLanguageLoader`] (`LOADER`) seeded with the
//!    fallback language (`en`) at first access and re-selected against
//!    the user's preferred locales by [`init`].
//! 3. The [`t!`] / [`t_args!`] macros that call
//!    [`FluentLanguageLoader::get`] / [`FluentLanguageLoader::get_args`]
//!    at runtime, so callers in `dbboard-ui` do not need to thread the
//!    loader through every call site. We deliberately do *not* use
//!    `i18n_embed_fl::fl!`: that proc-macro resolves `i18n.toml` and
//!    `<crate>.ftl` against the *calling* crate's `CARGO_MANIFEST_DIR`,
//!    which would force every consumer to duplicate the embed config.
//!
//! Locale resolution priority (highest first):
//!
//! 1. An explicit `override_lang` argument to [`init`] (used by tests
//!    and the `DBBOARD_LANG` env-var path in `apps/dbboard`).
//! 2. [`sys_locale::get_locale`] — the OS-reported user locale.
//! 3. The fallback chain inside `i18n-embed`, which terminates at `en`.
//!
//! `DbError` text is intentionally **not** translated — it crosses the
//! HTTP contract (ADR-0009). Only presentation labels live here.

use i18n_embed::{
    fluent::{fluent_language_loader, FluentLanguageLoader},
    LanguageLoader,
};
use rust_embed::RustEmbed;
use std::sync::OnceLock;
use unic_langid::LanguageIdentifier;

/// Environment variable that overrides OS locale detection. Same idiom
/// as `DBBOARD_PG_URL` / `DBBOARD_D1_*` env precedence elsewhere in the
/// app.
pub const ENV_LOCALE_OVERRIDE: &str = "DBBOARD_LANG";

/// Walks `crates/dbboard-i18n/i18n/` at compile time. Every locale
/// folder containing a `dbboard.ftl` is an available language.
#[derive(RustEmbed)]
#[folder = "i18n/"]
struct Localizations;

/// The global Fluent loader. Initialised on first access with the
/// fallback language only; [`init`] re-selects it for the user's
/// preferred locales. `OnceLock` instead of `LazyLock` to stay within
/// MSRV 1.75 (`LazyLock` is 1.80+).
static LOADER: OnceLock<FluentLanguageLoader> = OnceLock::new();

fn loader_init() -> &'static FluentLanguageLoader {
    LOADER.get_or_init(|| {
        let loader = fluent_language_loader!();
        loader.load_fallback_language(&Localizations).expect(
            "dbboard-i18n: failed to load the en fallback language; \
             this is a build-time invariant — i18n/en/dbboard-i18n.ftl \
             must exist",
        );
        loader
    })
}

/// Borrow the global loader. Re-exported for the [`t!`] macro and for
/// `dbboard-ui` views that need to feed the loader into other widgets.
#[must_use]
pub fn loader() -> &'static FluentLanguageLoader {
    loader_init()
}

/// Languages this build ships translations for. Discovered at runtime
/// from the embedded folder, so adding a new locale only requires
/// dropping in a `i18n/<tag>/dbboard-i18n.ftl` and rebuilding.
#[must_use]
pub fn available_languages() -> Vec<LanguageIdentifier> {
    loader_init()
        .available_languages(&Localizations)
        .unwrap_or_default()
}

/// Build the requested-locale list per ADR-0015 priority. Public so
/// tests can exercise the resolution rules without touching env vars
/// or selecting the loader.
///
/// Resolution rule:
/// - If `override_lang` (or the `DBBOARD_LANG` env var) parses as a
///   valid BCP-47 tag, that tag is the *only* request. An unknown but
///   well-formed tag (e.g. `xx`) falls through to the loader's `en`
///   fallback — exactly what the operator asked for.
/// - If the override is missing or malformed, fall back to the OS
///   locale via [`sys_locale::get_locale`].
/// - If nothing parses, return an empty list — `i18n_embed::select`
///   will then use the loader's hard-coded fallback (`en`).
#[must_use]
pub fn requested_locales(override_lang: Option<&str>) -> Vec<LanguageIdentifier> {
    let raw_override = override_lang
        .map(str::to_owned)
        .or_else(|| std::env::var(ENV_LOCALE_OVERRIDE).ok());

    if let Some(raw) = raw_override {
        if let Ok(parsed) = raw.trim().parse::<LanguageIdentifier>() {
            // Authoritative — do not also append OS locale, or the
            // override stops being an override the moment its tag is
            // not a shipped locale.
            return vec![parsed];
        }
        // Malformed override: fall through to OS detection rather
        // than crashing.
    }

    if let Some(os) = sys_locale::get_locale().and_then(|s| s.parse().ok()) {
        return vec![os];
    }

    Vec::new()
}

/// Resolve the user's locale and select it on the global loader. Safe
/// to call more than once — later calls reselect without rebuilding
/// the bundle cache. Returns the active loader for convenience.
///
/// # Errors
///
/// Propagates [`i18n_embed::I18nEmbedError`] from the underlying
/// `select`. The most common cause is a malformed `.ftl` file shipped
/// in the embed.
pub fn init(
    override_lang: Option<&str>,
) -> Result<&'static FluentLanguageLoader, i18n_embed::I18nEmbedError> {
    let loader = loader_init();
    let requested = requested_locales(override_lang);
    // `select` walks `requested` against `available_languages()` and
    // applies fallback (zh-CN → zh → en, pt-BR → pt → en, etc).
    i18n_embed::select(loader, &Localizations, &requested)?;
    Ok(loader)
}

/// Re-exports used by the [`t!`] / [`t_args!`] macros. Kept under
/// `__private` so callers do not depend on `fluent-bundle` types
/// transitively.
#[doc(hidden)]
pub mod __private {
    pub use fluent_bundle::FluentValue;
    pub use std::collections::HashMap;
}

/// Lookup a translation key with no arguments.
///
/// Resolves at runtime against the global loader; missing keys return
/// the key string verbatim (Fluent's default), which surfaces typos in
/// developer builds without crashing the UI.
///
/// ```ignore
/// use dbboard_i18n::t;
/// let label = t!("tables-heading");
/// ```
#[macro_export]
macro_rules! t {
    ($id:literal) => {
        $crate::loader().get($id)
    };
}

/// Lookup a translation key that takes Fluent arguments.
///
/// ```ignore
/// use dbboard_i18n::t_args;
/// let label = t_args!("history-title", count = 12);
/// ```
#[macro_export]
macro_rules! t_args {
    ($id:literal, $($arg_name:ident = $arg_val:expr),+ $(,)?) => {{
        let mut args: $crate::__private::HashMap<
            &'static str,
            $crate::__private::FluentValue<'static>,
        > = $crate::__private::HashMap::new();
        $(
            args.insert(stringify!($arg_name), $crate::__private::FluentValue::from($arg_val));
        )+
        $crate::loader().get_args_concrete($id, args)
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // The Fluent loader is a process-global. Cargo runs tests in
    // parallel, so any test that calls `init()` and then reads keys
    // must hold this mutex for the full init→read pair, or another
    // thread's `init()` can switch the loader's selection mid-read.
    static LOADER_GUARD: Mutex<()> = Mutex::new(());

    #[test]
    fn fallback_language_is_en() {
        let _g = LOADER_GUARD.lock().unwrap();
        let loader = loader();
        assert_eq!(loader.fallback_language().to_string(), "en");
    }

    #[test]
    fn available_languages_includes_en() {
        let _g = LOADER_GUARD.lock().unwrap();
        let langs: Vec<String> = available_languages()
            .iter()
            .map(ToString::to_string)
            .collect();
        assert!(
            langs.iter().any(|s| s == "en"),
            "en must always be a shipped locale (fallback). got: {langs:?}"
        );
    }

    #[test]
    fn explicit_override_takes_priority() {
        let requested = requested_locales(Some("ja"));
        assert!(
            requested
                .first()
                .is_some_and(|li| li.to_string().starts_with("ja")),
            "override_lang should win. got: {requested:?}"
        );
    }

    #[test]
    fn override_is_authoritative_single_entry() {
        // ja override must not also append the OS locale — that would
        // make the override stop being an override the moment its tag
        // is not a shipped locale.
        let requested = requested_locales(Some("ja"));
        assert_eq!(requested.len(), 1);
    }

    #[test]
    fn malformed_override_is_ignored() {
        // "!!" does not parse as BCP-47; resolution should fall through
        // to OS detection rather than panicking.
        let _ = requested_locales(Some("!!"));
    }

    #[test]
    fn init_with_en_resolves_to_en_keys() {
        let _g = LOADER_GUARD.lock().unwrap();
        let loader = init(Some("en")).expect("init must succeed for en");
        let v = loader.get("tables-heading");
        assert_eq!(v, "Tables");
    }

    #[test]
    fn init_with_ja_resolves_to_ja_keys() {
        let _g = LOADER_GUARD.lock().unwrap();
        let loader = init(Some("ja")).expect("init must succeed for ja");
        let v = loader.get("tables-heading");
        // Translation is "テーブル" — we only assert non-empty + not
        // equal to the en source so this test does not break on minor
        // wording revisions.
        assert!(!v.is_empty());
        assert_ne!(v, "Tables", "ja should differ from en");
    }

    #[test]
    fn unknown_locale_falls_back_without_error() {
        let _g = LOADER_GUARD.lock().unwrap();
        // "xx" is a syntactically valid BCP-47 tag but not a shipped
        // locale. select() should fall back to en silently.
        let loader = init(Some("xx")).expect("init must not error on unknown locale");
        let v = loader.get("tables-heading");
        assert_eq!(v, "Tables", "fell back to en");
    }
}
