#!/usr/bin/env sh
#
# Install the repository's git hooks.
#
# The hooks used to arrive on their own: cargo-husky was a dev-dependency and
# copied .cargo-husky/hooks/ into .git/hooks/ on the first `cargo test`. It was
# dropped from the manifests when the workspace was restructured, and the
# documentation kept saying otherwise, so hooks stopped following their source.
# One machine ran a pre-push hook that was three weeks behind the file it was
# supposedly installed from.
#
# Run this after cloning, and after editing anything in .cargo-husky/hooks/.
# crates/dbboard-config/tests/hook_install_drift.rs fails when you forget.

set -e

src=".cargo-husky/hooks"
dst=".git/hooks"

if [ ! -d "$src" ]; then
    echo "error: $src not found — run this from the repository root." >&2
    exit 1
fi

if [ ! -d "$dst" ]; then
    echo "error: $dst not found — is this a git checkout?" >&2
    exit 1
fi

for hook in "$src"/*; do
    name=$(basename "$hook")
    cp "$hook" "$dst/$name"
    chmod +x "$dst/$name"
    echo "installed $name"
done
