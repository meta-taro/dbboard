# Internal distribution (maintainer runbook)

How to cut an internal test build of dbboard and hand it to someone —
whether a data-collection operator or an internal tester — on Windows.

This is the **producer** side. The **recipient** side is documented
separately:

- A data-collection operator setting up the three fixed connections:
  [`../collector-setup/README.md`](../collector-setup/README.md).
- A general internal tester trying the app and reporting back:
  [`../internal-testing.md`](../internal-testing.md).

> **Audience:** the maintainer. **Platform:** Windows-only for now
> (ADR-0032). No maintainer-run SaaS exists — distribution is a hand-off
> of files, not a hosted service.

---

## What you hand over

The whole handoff is **three things**, delivered over **two channels**:

| Item | What it is | Channel |
| --- | --- | --- |
| `dbboard.exe` | The self-contained release binary (no installer needed). | Ordinary (file share, USB, chat). |
| `<name>.dbbx` | An encrypted bundle carrying every connection **and** its secrets (ADR-0038). Optional — only when you want to seed the recipient's connections for them. | Ordinary — it is passphrase-encrypted. |
| The passphrase | The phrase that decrypts the `.dbbx`. | **Separate, out-of-band** (spoken, phone, a different app). Never in the same message as the file. |

If you are **not** pre-seeding connections (e.g. a tester who will point
the app at their own database), skip the `.dbbx` and the passphrase — just
hand over the exe and point them at the recipient guide.

---

## Step 1 — Build the release exe

From a clean checkout of the branch you want to ship (normally `develop`):

```powershell
cargo build --release
```

The binary lands at `target\release\dbboard.exe`. It is hardened for
distribution already (ADR-0032): GUI subsystem (no console window),
embedded icon and version metadata, and statically linked CRT (no VC++
redistributable needed on the target machine).

Sanity-check what you built before shipping it:

```powershell
# Confirm the version metadata is what you expect.
(Get-Item .\target\release\dbboard.exe).VersionInfo | Format-List FileVersion, ProductName

# Confirm no real connection names leaked into the binary (should print nothing).
# Fill $realNames from your PRIVATE notes — never hard-code them in a tracked file.
$realNames = @()  # e.g. @('<real-name-1>','<real-name-2>')
if ($realNames) {
    Select-String -Path .\target\release\dbboard.exe -Pattern $realNames -Encoding Byte -SimpleMatch -Quiet
}
```

> The repo was sanitized so the real business connection names live only in
> the maintainer's **private** notes, never in a tracked file (including
> this one). Paste them into `$realNames` locally to run the leak check; the
> check prints nothing when the exe is clean.

Optionally stage the handoff into `dist\` (ignored by git):

```powershell
New-Item -ItemType Directory -Force -Path .\dist | Out-Null
Copy-Item .\target\release\dbboard.exe .\dist\
```

## Step 2 — (Optional) Export an encrypted bundle

Do this only when you want the recipient's connections seeded for them —
the "fast path" that replaces hand-editing a template and seeding secrets
with `cmdkey`.

Requirements: **your own** dbboard must already have the connections
working, because the export resolves every connection's secret from your
machine's Credential Manager and fails if any is missing.

1. Launch your dbboard, open the connection window, click **Export**.
2. Choose a passphrase (minimum 8 characters) and confirm it.
3. Save the `.dbbx` (into `dist\` if you are staging there).

The result is an `age` passphrase-encrypted file (scrypt KDF +
ChaCha20-Poly1305). It is safe to send over an ordinary channel **as long
as the passphrase travels separately**.

> **If Export fails with a missing-secret error**, one of your own
> connections has no secret seeded in this machine's Credential Manager.
> Export is all-or-nothing: seed the missing secret (see the collector
> guide's Step 2) and retry, or delete the connection you do not intend to
> ship before exporting.

## Step 3 — Deliver

1. Send `dbboard.exe` (and the `.dbbx` if you made one) over the ordinary
   channel.
2. Send the passphrase over a **different** channel. Do not put the file
   and its passphrase in the same message or thread.
3. Point the recipient at the right guide:
   - operator seeding the three fixed connections →
     [`../collector-setup/README.md`](../collector-setup/README.md)
     (its "Fast path — import an encrypted bundle" section covers the
     `.dbbx` import);
   - tester trying the app → [`../internal-testing.md`](../internal-testing.md).

---

## Do-not-commit hygiene

This repository is **public**. The following must never land in a commit;
`.gitignore` already blocks them, but confirm `git status` is clean of
them before you push:

- **`*.dbbx`** — encrypted bundles. Even encrypted, they carry secret
  material; they are export artifacts, not source.
- **`/dist/`** — the staging directory and everything in it (exe, bundle,
  copies of the guides).
- **`connections.toml`** — a real connection file with live ids and
  keyring references. Only `*.template.toml` (neutral placeholders) is
  tracked.
- The private **real→placeholder name mapping** for the three collector
  connections. It lives only in the maintainer's private notes, never in
  the repo, commit messages, or these docs.

The exe itself is never committed (it is under `target\`, already
ignored), and it embeds no secrets — secrets live only in the recipient's
OS Credential Manager after they import the bundle or seed them by hand.

---

## How it fits together

- Windows exe hardening (console suppression, icon/metadata, CRT-static):
  ADR-0032.
- The encrypted `.dbbx` bundle export/import: ADR-0038.
- Secret-reference indirection (the config file never holds secrets):
  ADR-0013 / ADR-0024.
- Self-host-only distribution posture (no maintainer-run SaaS): recorded
  in project memory / the web-side ADR.
