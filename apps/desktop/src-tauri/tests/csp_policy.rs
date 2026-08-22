//! The webview's Content-Security-Policy, guarded against silent regressions.
//!
//! A wrong CSP does not fail the build. The app launches and then some part of
//! it stops working, with the only evidence in a webview console nobody has
//! open (#210). These tests pin the three properties that are both easy to
//! break by accident and invisible when broken.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
}

/// `default-src 'self'; script-src 'self'` -> `{"default-src": ["'self'"], ...}`.
fn directives(csp: &str) -> BTreeMap<String, Vec<String>> {
    csp.split(';')
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(|directive| {
            let mut parts = directive.split_whitespace();
            let name = parts.next().expect("non-empty directive has a name");
            (
                name.to_ascii_lowercase(),
                parts.map(str::to_string).collect(),
            )
        })
        .collect()
}

fn configured_csp() -> String {
    let conf: serde_json::Value = serde_json::from_str(&read(&crate_dir().join("tauri.conf.json")))
        .expect("tauri.conf.json is valid JSON");
    conf["app"]["security"]["csp"]
        .as_str()
        .expect(
            "app.security.csp must be a policy string; `null` means Tauri injects no CSP \
             at all and every remote source is allowed (#210)",
        )
        .to_string()
}

/// Strip `//` line comments so a directive named in prose does not count as
/// configuration. Block comments are not used in the files this reads.
fn code_only(source: &str) -> String {
    source
        .lines()
        .map(|line| match line.find("//") {
            Some(i) => &line[..i],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn csp_is_set_and_does_not_re_open_script_execution() {
    let csp = configured_csp();
    let d = directives(&csp);

    assert_eq!(
        d.get("default-src").map(Vec::as_slice),
        Some(["'self'".to_string()].as_slice()),
        "default-src is the floor every unlisted fetch directive falls back to; \
         widening it widens all of them at once. csp = {csp}"
    );

    let script_src = d
        .get("script-src")
        .unwrap_or_else(|| panic!("script-src must be stated explicitly. csp = {csp}"));
    assert!(
        script_src.contains(&"'self'".to_string()),
        "script-src must allow the bundle's own modules. csp = {csp}"
    );
    // The whole point of the policy is to stop injected markup from executing.
    // 'unsafe-inline' or 'unsafe-eval' here would leave it worth nothing.
    for forbidden in ["'unsafe-inline'", "'unsafe-eval'", "*"] {
        assert!(
            !script_src.iter().any(|s| s == forbidden),
            "script-src must not contain {forbidden}: the two inline scripts in \
             the built index.html are hashed by tauri-codegen at build time, so \
             no blanket allowance is needed. csp = {csp}"
        );
    }

    for (directive, expected) in [
        ("object-src", "'none'"),
        ("base-uri", "'self'"),
        ("frame-ancestors", "'none'"),
        ("form-action", "'none'"),
    ] {
        assert_eq!(
            d.get(directive).map(Vec::as_slice),
            Some([expected.to_string()].as_slice()),
            "{directive} must be {expected}; default-src does not cover it. csp = {csp}"
        );
    }

    // Every backend call goes over Tauri's IPC custom protocol, which the
    // webview reaches with `fetch`. Windows and Android use
    // `http://ipc.localhost`; the other platforms use `ipc://localhost`. Miss
    // one and the IPC silently falls back to the slower postMessage path.
    let connect_src = d
        .get("connect-src")
        .unwrap_or_else(|| panic!("connect-src must be stated explicitly. csp = {csp}"));
    for source in ["'self'", "ipc:", "http://ipc.localhost"] {
        assert!(
            connect_src.iter().any(|s| s == source),
            "connect-src must allow {source}. csp = {csp}"
        );
    }
}

/// CodeMirror's `style-mod` mounts its themes by creating a `<style>` element
/// and assigning `textContent` at runtime. That has no nonce and no hash a
/// build step could compute, so `style-src` has to carry `'unsafe-inline'`.
///
/// The trap: under CSP3, `'unsafe-inline'` is *ignored* as soon as the same
/// directive carries a nonce or a hash. Tauri adds a style nonce for every
/// `<style>` element it finds in the HTML it embeds. So adding a single
/// `<style>` block to `app.html` would silently disable `'unsafe-inline'` and
/// leave the editor unstyled — with nothing failing at build time.
#[test]
fn style_src_stays_usable_for_codemirror() {
    let csp = configured_csp();
    let d = directives(&csp);

    let style_src = d
        .get("style-src")
        .unwrap_or_else(|| panic!("style-src must be stated explicitly. csp = {csp}"));
    assert!(
        style_src.contains(&"'unsafe-inline'".to_string()),
        "style-src needs 'unsafe-inline' for CodeMirror's runtime-injected \
         stylesheet. csp = {csp}"
    );

    let app_html = read(&crate_dir().join("../src/app.html"));
    assert!(
        !app_html.to_ascii_lowercase().contains("<style"),
        "app.html must not contain a <style> element: Tauri would add a nonce to \
         style-src, and a nonce makes CSP3 ignore the 'unsafe-inline' CodeMirror \
         depends on. Put the rules in a .css file instead."
    );
}

/// SvelteKit's `kit.csp` emits its own `<meta http-equiv>` policy. Two policies
/// on one document intersect, and SvelteKit only knows about the scripts it
/// emits itself — not the hand-written theme script in `app.html`. Tauri
/// already hashes both, so enabling `kit.csp` can only subtract.
#[test]
fn sveltekit_does_not_emit_a_second_policy() {
    let config = code_only(&read(&crate_dir().join("../svelte.config.js")));
    assert!(
        !config.contains("csp"),
        "svelte.config.js must not set kit.csp; Tauri is the single source of \
         the policy (#210)"
    );
}
