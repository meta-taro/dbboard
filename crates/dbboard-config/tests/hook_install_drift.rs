//! The installed hooks must match the ones in the repository.
//!
//! `.cargo-husky/hooks/` is the source; `.git/hooks/` is what git actually
//! runs. Nothing keeps them in step. cargo-husky used to — it was a
//! dev-dependency that copied the directory across on the first `cargo test` —
//! but it was dropped when the workspace was restructured, and five documents
//! went on saying the hooks install themselves. They do not.
//!
//! The gap is invisible while it matters most. Editing a hook looks like it
//! took effect, because the file you edited is the one you read back; git keeps
//! running the copy. A pre-push hook here was three weeks behind its source
//! that way, and a fix written into `.cargo-husky/hooks/pre-push` would have
//! done nothing at all.
//!
//! So the copy is compared against the source. Run `sh scripts/install-hooks.sh`
//! when this fails.
//!
//! A checkout with no installed hooks passes: CI has none by design (ADR-0104
//! moved the real gate into the workflow), and a fresh clone should not fail
//! its first `cargo test` over a convenience it has not been offered yet.

use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    // crates/dbboard-config -> crates -> <root>
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("this crate sits two directories below the workspace root")
        .to_path_buf()
}

/// Compared with line endings normalised: the working tree is CRLF on Windows
/// and LF elsewhere, and a hook that differs only in that runs identically.
fn normalised(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
        .replace("\r\n", "\n")
}

#[test]
fn installed_hooks_match_their_source() {
    let root = workspace_root();
    let src = root.join(".cargo-husky/hooks");
    let dst = root.join(".git/hooks");

    // `.git` is a file, not a directory, inside a worktree; and CI checkouts
    // carry no hooks. Neither is drift.
    if !dst.is_dir() {
        return;
    }

    let sources: Vec<_> = std::fs::read_dir(&src)
        .unwrap_or_else(|e| panic!("read {}: {e}", src.display()))
        .map(|e| e.expect("a readable directory entry").path())
        .filter(|p| p.is_file())
        .collect();

    let mut stale = Vec::new();
    let mut missing = Vec::new();

    for hook in &sources {
        let name = hook
            .file_name()
            .expect("a file has a name")
            .to_string_lossy()
            .into_owned();
        let installed = dst.join(&name);

        if !installed.exists() {
            missing.push(name);
        } else if normalised(&installed) != normalised(hook) {
            stale.push(name);
        }
    }

    // All of them absent is an un-installed checkout, not drift. Some of them
    // absent means an install that stopped halfway or a hook added since.
    if missing.len() == sources.len() {
        return;
    }

    assert!(
        stale.is_empty() && missing.is_empty(),
        ".git/hooks is out of step with .cargo-husky/hooks.\n\
         stale (installed copy differs): {stale:?}\n\
         never installed: {missing:?}\n\
         Run `sh scripts/install-hooks.sh`. Until you do, git runs the old \
         copy and edits to the source have no effect."
    );
}
