# Changelog

All notable changes to **dbboard** are documented here. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the
project follows [SemVer](https://semver.org/spec/v2.0.0.html), where the
public API is the HTTP contract in
[`docs/api-contract.md`](docs/api-contract.md) (see
[ADR-0011](docs/decisions.md)).

## [Unreleased]

## [0.15.0] — 2026-09-03 — The app has its own name

### Changed

- **The application is called dbboard.** It was `dbboard-desktop` everywhere a
  person could see it — the Dock, the `.app`, the installer, the Start menu —
  because that is the cargo package's name, chosen so it could be told apart
  from `dbboard-core` and `dbboard-mcp`. None of that is the user's problem:
  there is one desktop client and it is dbboard. The `.dmg` and the Windows
  setup are now `dbboard_<version>_…`, and the download page recognises both
  spellings so older releases keep offering their builds.

  Two consequences worth knowing. A **Windows** install from v0.14.0 or
  earlier is not upgraded in place — the installer keys on the product name,
  so the new one lands beside it and the old entry needs uninstalling by hand.
  On **macOS** the in-app updater replaces the bundle where it stands, so an
  existing install shows the new name in the Dock while its folder is still
  `dbboard-desktop.app`; installing from the `.dmg` again tidies that.
  **No connection, setting or stored credential is affected** — those hang off
  the bundle identifier, which has not changed. See
  [ADR-0143](docs/decisions.md).

### Fixed

- **A bundle that will not open now says which check refused it.** `.dbbx`
  exists to be carried to another machine, and until now every structural
  failure arrived as the same four words — "bundle is corrupt or was not
  produced by dbboard". Three different things produce that: the bytes are not
  an age file, the key stanza will not unwrap, or the body fails its
  authentication tag. They call for three different next steps, so the error
  now carries the stage that refused the bundle and what `age` itself said.

- **A truncated bundle is no longer reported as an I/O error.** An interrupted
  copy is the most likely way a bundle gets damaged in real use, and `age`
  reports the short read as I/O. But decryption is handed a byte slice — there
  is no device that can fault — so "bundle I/O error: failed to fill whole
  buffer" was a diagnosis of something that cannot have happened, pointing the
  reader at their disk instead of at the copy. A bundle that stops early is
  now reported as corrupt at the stage it stopped.

### Changed

- **A finding from v0.14's baseline is withdrawn.** That release's notes said
  `annotations.toml` parses "six times slower than `connections.toml` for the
  same twenty connections". The two files are not the same document: the
  annotations fixture carries a note per table and per column on top of those
  connections, so it is 38,952 bytes against 2,092 — 18.6x the data parsed in
  6x the time, which makes it the faster of the two per byte. Measured
  directly, `toml` costs 21 ns/byte on one and 19.5 ns/byte on the other.
  Nothing was slow; the comparison was invalid. A test now asserts the two
  fixtures are *not* the same size, so the same wrong reading cannot be taken
  from the same two numbers again.

  The other two leads were checked as well. `truncate_rows` is a
  `Vec::truncate`, and its 570 µs is the cost of freeing 9,900 rows — which
  every adapter already avoids paying by capping at the source. `query_10k`
  splits into 143 ns per row and 37 ns per value, which is what the row
  representation costs rather than a mistake in it. So no optimisation ships
  against the current measurement points, and the effort moves to not building
  ten thousand rows for a grid that shows fifty. See
  [ADR-0142](docs/decisions.md).

## [0.14.0] — 2026-09-02 — Speed, measured

### Added

- **The speed the slot is named after now has numbers behind it.** There were
  no benchmarks in this workspace at all — no `benches/`, no criterion,
  nothing — while the README opened by calling dbboard "high-performance".
  `crates/dbboard-bench` measures the three areas `docs/roadmap.md` reserved
  this slot for and writes
  [`docs/performance-baseline.md`](docs/performance-baseline.md): the config
  work that happens before a window appears, opening a database and reading
  enough of it to show a table, and carrying a 10,000-row result to the
  frontend. Measurement first, on purpose — optimising before it means the
  next regression has nothing to be a regression against.

  The first run says the in-process work is not where the time goes. Every
  browse operation is single-digit microseconds; a full 10,000-row result
  serialises for the Tauri IPC boundary in under a millisecond, while pulling
  the same rows out of the driver costs 4.4 ms — so the JSON hop that looked
  like the obvious suspect is the cheaper half. Two things did stand out:
  `truncate_rows` spends 570 µs discarding 9,900 rows, over half what
  serialising all of them costs, and parsing `annotations.toml` runs six times
  slower than `connections.toml` for the same twenty connections.

  No number here is asserted by a test. A threshold on a timing fails on a busy
  runner rather than on a regression, and this project has already paid for one
  rare intermittent failure (ADR-0125). What is asserted is the *set* of
  measurement points, because deleting one otherwise looks exactly like the
  thing it measured getting faster. See
  [ADR-0141](docs/decisions.md).

- **An agent can pack connections for another machine, without ever holding
  the key.** Moving a set-up to a new laptop has been a bundle file since
  0.4.0, but making one is five steps by hand: pick the connections, pick a
  directory, invent a passphrase, save, and put the passphrase somewhere it
  survives the trip. `dbboard-mcp` gains `export_connections`, which does the
  first four. It seals only the connections you name — there is no form of it
  that exports everything — and answers with the file path, a count, and the
  **name of a credential-manager slot**. Not the passphrase: dbboard generates
  one, files it under `dbboard.export.<name>` on this machine, and wipes its
  copy. A tool result is plaintext in an agent's transcript, so a bundle and
  the key to it must not travel together.

  The switch is a directory, not a flag. Without an `[mcp_export]` table in
  `connections.toml` naming one, the call is refused permanently — and that
  table is in the connection store rather than in an environment variable
  because an agent usually owns the file its own MCP server is launched from,
  and a permission it can grant itself is not a permission. Choosing the
  directory *is* the grant, so there is no "on" left switched on after the
  laptop is provisioned. A directory that is not there is refused rather than
  created. The result reports how many entries carried a secret this machine
  cannot read, but not which ones, so a connection hidden behind an alias
  stays hidden. See [ADR-0140](docs/decisions.md).

### Fixed

- The setup instructions built a different machine than the one CI builds.
  ADR-0139 pinned the toolchain to an exact version because `clippy
  -D warnings` makes the lint set part of the build, but Requirements went on
  asking for "Rust stable (latest)" — and `rustup` reads
  `rust-toolchain.toml` regardless, so anyone following the README by hand
  installed a compiler nothing ever mentioned. The clone URL was still
  `<your-org>`, and the smoke test was `cargo test`, which on Windows tears
  down two in-memory libSQL databases at once and crashes after every
  assertion has passed; `scripts/cargo-test-serialised.sh` exists precisely
  to avoid that and is what the mandatory commands use. A fourth test in
  `toolchain_pin_drift.rs` now reads the pin and fails when the README does
  not name it.

## [0.13.0] — 2026-08-26 — Knowing what changed, and letting an agent tidy up

### Added

- **An agent can tidy the connection list.** The mark and the order that
  arrived in 0.12.0 were editable only by hand, one row at a time — which is
  the wrong shape of work for a person and the right shape for an agent, and
  the operator who most needs twenty connections sorted is the one who has
  twenty. `dbboard-mcp` gains `set_connection_mark` and `move_connection`,
  and `list_connections` now reports each entry's `position` alongside its
  `color` and `tag` so a sort is aimed at the list that exists rather than at
  one read a move ago. Neither tool is behind `mcp_write`: that switch is
  about the data in a database, and putting a sidebar colour behind it would
  mean granting production write access to have a list sorted. See
  [ADR-0136](docs/decisions.md).
- **The About dialog says what each version changed.** The version number was
  all it showed, and the only description a release ever gave came from the
  update dialog — once, at the moment the update was offered. Anyone who took
  an update without reading it, or who skipped a version because two arrived
  as one step, had no way back to the text. About now shows the notes for the
  version it is running, with every earlier version reachable from the same
  picker. They are read from the `CHANGELOG.md` the build was cut from, so
  there is no second list to keep current, and the file travels inside the
  binary rather than being fetched — the dialog works with no network and
  cannot describe a release you are not running. The notes are in English,
  and a line under them says so where the interface is not. See
  [ADR-0137](docs/decisions.md).

## [0.12.0] — 2026-08-25 — A connection list you can steer

### Added

- **The connection list keeps the order you put it in.** Entries were
  rendered in the order they happened to be added, so the ones opened every
  day sat wherever they had landed. A row is now moved by dragging its handle, or
  by focusing the handle and pressing `↑`/`↓`. The order is the order of
  `[[connections]]` in the file: no new key, older builds read it unchanged,
  and it travels inside a `.dbbx` bundle for free. The handle is disabled
  while a filter is hiding rows — "below the next visible row" is a different
  feature, and moving a connection past something the operator cannot see is
  a silent wrong answer. The drag is built on pointer events rather than the
  browser's own drag-and-drop, so the window keeps the OS-level drop that
  dropping a bundle onto it will need. See
  [ADR-0127](docs/decisions.md) and [ADR-0128](docs/decisions.md).
- **A connection can be found by typing instead of by scanning.** The
  manager's list had no way in but the eye, and the app's only search box
  filters tables and columns, one panel below. Name and id are matched, every
  typed word having to match, so a second word narrows. The kind is
  deliberately not matched: typing `my` to reach "my shop" would otherwise
  return every MySQL row, which is the opposite of narrowing. The id is
  matched because it is the one handle that can be pasted in from a log.
  Nothing is persisted — this is a way of finding one row, not a saved view.
- **A connection can be marked with a colour and a short tag.** "Which server
  am I about to run this against?" had only the connection's name to answer
  it, and names are chosen to be descriptive rather than distinguishable at a
  glance: a production store and the copy made to repair it read as the same
  thing when you are already looking at a result grid. The mark is two halves
  and both are required together — a colour from a fixed palette of eight,
  with a value in every theme, and a tag of at most twelve characters that
  the operator writes (`prod`, `本番`, `検証環境`). The tag is the half that
  carries the meaning: colour alone is lost by a colour-blind reader, by a
  greyscale screenshot pasted into an issue, and by a screen reader alike. A
  duplicate inherits neither half, because the mark exists to tell production
  from a copy of it. See [ADR-0126](docs/decisions.md).
- **A mark can be set from the list, and the colour now sits at the head of
  the row.** Marking was only reachable from the edit form, which also holds
  the DSN, the tunnel and the AI-agent permissions: recolouring six
  connections meant six whole-connection saves for a decision about nothing
  but appearance. Right-clicking a row in the sidebar now opens a swatch
  grid — one click paints the row, and the tag is typed beside it. The
  colour renders as a bar at the left edge rather than as a pill after the
  name, because down a list the left edge is the one column the eye can scan
  without reading; the bar holds its width on unmarked rows so that marking
  one does not shift its name sideways. A colour with no tag is accepted
  here, unlike in the form, since the bar sits on a row that already carries
  the connection's name. See [ADR-0130](docs/decisions.md).
- **The line between the connections and the tables can be moved.** It was
  fixed, and the connection list above it had no height of its own: three
  connections left a boundary sitting near the top of the sidebar, and twenty
  pushed the table list off the bottom of the window with no way to bring it
  back. The line is now dragged up and down, and double-clicking it hands the
  split back to the number of connections registered — which is what "back to
  where it belongs" means when the number of rows is not fixed. Until the line
  is dragged for the first time it keeps following that count on its own, so
  adding a connection makes room for itself. The connection list scrolls once
  it reaches its ceiling instead of growing without limit. Arrow keys move the
  line and `Home` resets it. See [ADR-0131](docs/decisions.md).

- **An AI agent can register a local database itself.** Setting up a SQLite
  file and then asking a person to retype its path into a dialog is two jobs
  where there is one. The MCP server gains `add_connection`, which writes the
  entry into `connections.toml` directly. Only the kinds that store *nothing*
  in the keychain can be registered this way — a SQLite/libSQL file on this
  machine, and the local Firestore emulator, which authenticates with a fixed
  token and so has no service account to save. Everything that needs a
  password, a token or a key is refused, and the refusal is in the tool's own
  description rather than in an error, so an agent learns it before sending
  the credential rather than after. The new connection is created read-only:
  agent writes stay off, and only a person can turn them on. See
  [ADR-0134](docs/decisions.md).

### Fixed

- **A half-typed connection is no longer thrown away by a stray click.**
  While the add or edit form was open, clicking anywhere outside the dialog
  closed it and discarded every field, and `Escape` did the same from the
  keyboard the fields were being typed on. On a small window — where the panel
  covers most of what is behind it — reaching past it to see the details being
  copied was the same gesture as deleting the work, so registration could not
  be completed at all. The backdrop now does nothing while any panel is open,
  and `Escape` steps back to the list only from a form that has not been typed
  into. Leaving is unchanged and still one deliberate click: the ✕ in the
  header, or Cancel in the form. Import, export, duplicate and repair are
  covered on the same terms. See [ADR-0132](docs/decisions.md).
- **The connection dialog can be moved off whatever it is covering.** It was
  fixed in the centre of the window with no way to reposition it, so on a
  laptop screen there was no way to read what was behind it. Drag its header
  to move it, double-click the header to put it back in the centre — the same
  gesture the sidebar divider already uses. It cannot be dragged out of reach:
  a strip stays on screen sideways, the header never passes the top or bottom
  edge, and shrinking the window pulls a moved dialog back into view. See
  [ADR-0132](docs/decisions.md).
- **The reorder handle no longer offers a drag it will refuse.** While a
  filter was hiding rows the handle correctly declined to move anything, but
  still brightened under the pointer and answered a press with a closed-fist
  cursor. Both were single-class CSS rules that outranked the disabled state.

- **A connection added outside a running dbboard is no longer erased by it.**
  The window read `connections.toml` once at startup and wrote the whole file
  back on every change, so an entry added by hand — or, now, by an agent —
  while dbboard was open disappeared at the next rename, with no error and no
  conflict. The duplicate-id guard had the same blind spot and let an id
  already taken on disk through. Every write now re-reads the file first. See
  [ADR-0133](docs/decisions.md).
- **An update that does not install now says so on the next launch.** On one
  laptop the 0.10.0 → 0.11.0 update downloaded, dbboard quit, and nothing came
  back; started again by hand it was 0.10.0 with no sign anything had been
  attempted, and the same update was offered again. The attempt is now written
  down before the installer is handed control, and read back at startup: if the
  running build is still the one being replaced, the notice says the last update
  did not finish and offers the download page to install by hand. It reports
  once, is read before the update check so it still appears with no network, and
  says nothing when the update did land. Why that install stalls is a separate
  question and is not answered here. See [ADR-0135](docs/decisions.md).

## [0.11.0] — 2026-08-24 — Connection repair and duplication

### Added

- **A connection can be duplicated.** The connection list gains a
  *Duplicate* action that copies an entry's settings and its saved
  secrets into a new connection with saved-secret slots of its own. Making
  a second connection to the same database no longer means editing
  `connections.toml` by hand — which was also the only way to end up with
  the broken state below. The copy starts with no AI-agent alias and with
  agent writes off.
- **A connection that reads another connection's saved secret can be
  repaired, and is now visible before an export.** Since 0.10.0 dbboard
  could tell that an entry's saved-secret slot had been minted for a
  different connection, but only said so while writing a bundle, and
  offered no way out except editing the file by hand again. The state now
  shows in the connection list, and a *Repair* action gives the entry a
  slot of its own. It asks for the secret rather than copying the other
  connection's: reading a credential that belongs to something else, and
  writing a second copy of it into the keychain without saying so, is the
  thing the import guard already refuses. The other connection keeps its
  slot and its secret untouched. See
  [ADR-0118](docs/decisions.md).

### Fixed

- **Verification no longer crashes on a green branch.** On Windows,
  tearing down two in-memory libSQL databases at the same instant takes the
  whole test binary with it, after every assertion has passed. The
  workaround ran the adapter crate's tests one at a time — but four other
  crates reach that adapter through their dependencies and inherit the same
  hazard, and none of them were covered. The set is now derived from the
  dependency graph and checked by a test, so a crate that starts using the
  adapter cannot quietly drop off it. See
  [ADR-0119](docs/decisions.md).
- **Editing a git hook now takes effect.** The hooks were documented as
  installing themselves on first `cargo test`; the dependency that did that
  was dropped from the workspace some time ago and the documentation was
  not. `sh scripts/install-hooks.sh` installs them, and a test fails when
  the installed copies have fallen behind the ones in the repository. This
  affects contributors only: CI runs the same checks either way
  ([ADR-0104](docs/decisions.md)).
- **Hovering a connection now shows its name, not its internal id.** The
  sidebar cuts a long connection name off with an ellipsis, and the tooltip
  that would have shown the rest was showing the generated id instead — the
  one piece of text about a connection that appears nowhere else and that
  nobody has ever needed. Both the sidebar row and the toolbar pill now name
  the connection and its kind.

### Security

- **The desktop webview now runs under a Content-Security-Policy.** It had
  none: every remote script, style and connection the webview could name
  was allowed. Nothing in the app walks that path today — there is no
  place where a database value, an AI response or an export is rendered as
  HTML — so this changes no behaviour. It is the guard being put in place
  before the feature that needs it, rather than after. Scripts are limited
  to the bundle's own, whose two inline blocks the build hashes; network
  access is limited to Tauri's IPC, since every driver, provider and the
  updater already run in Rust. See [ADR-0120](docs/decisions.md).

## [0.10.0] — 2026-08-20

One database that could not be reached, and four things the program knew
but did not say.

`turso` had always meant a file on disk. The adapter was built with
libSQL's remote transport switched off, so a hosted database was not
awkward to open — it could not be opened at all, and one production
database stayed invisible while its siblings were all reachable.
`turso-remote` closes that hole, and closes it as a second kind rather
than a second meaning for the first, so nothing migrates and no existing
connection changes.

The rest is the same fault in four places. An import refused an entry to
avoid overwriting a live credential and then described the refusal as
"already present", followed by a hint — re-import with overwrite on —
that could not possibly change the outcome. An export wrote a bundle that
would be refused on the next machine and said nothing about it, on the
one machine where the problem could still be fixed. An MCP server several
releases behind answered exactly like a current one, so a bug that had
already been fixed was reported again. An update offered itself without
any account of what it changed. And an export proposed the same file name
twice, quietly asking to overwrite what, for a backup, is the only copy
of the older state.

None of those was wrong behaviour. Each of them left the person in front
of it guessing, and this release stops that.

In the same spirit: reaching a hosted database means a TLS stack, and the
one libSQL pins carries two advisories with no fixed version available.
They are narrow, and each one is listed in `deny.toml` with the reason it
cannot be fixed today and what would clear it, rather than left to be
found later.

### Added

- **Turso Cloud is reachable** — a new `turso-remote` connection kind
  ([ADR-0111](docs/decisions.md)), closing
  [#191](https://github.com/meta-taro/dbboard/issues/191). `turso` has
  always meant a libSQL file on disk, and the adapter was built with
  libSQL's remote transport switched off, so a `libsql://` URL could not
  be opened at all. `turso-remote` takes the endpoint the Turso dashboard
  shows plus an auth token, and also reaches a self-hosted `sqld` over
  `https://`, `http://`, `wss://` or `ws://`. The token is stored in the
  OS credential store like every other secret, never written to
  `connections.toml`, and never sent back to the edit form — blank means
  keep. `turso` is untouched, and nothing migrates: no existing `turso`
  connection can hold a URL, because one could never have been opened.
- **Exports no longer propose the same file name twice** — the save
  dialog now offers `dbboard-connections-20260819-163045.dbbx`,
  `dbboard-result-<stamp>.csv` and `<connection>-dump-<stamp>.sql`.
  Exporting twice used to land on an "overwrite?" prompt over the
  previous export — which, for a backup, is the only copy of the older
  state. The stamp is local time and sorts chronologically, so a
  directory of exports reads as a history.
- **An export says so when a connection carries someone else's saved-secret
  slot** ([ADR-0113](docs/decisions.md)), closing
  [#194](https://github.com/meta-taro/dbboard/issues/194). Importing such
  a bundle elsewhere refuses those entries — that check has existed since
  ADR-0038 — but it only fires on a machine that happens to hold the
  connection being pointed at, and by then the file is already written and
  handed over. The same fault is visible at export from the entry alone,
  because a slot name is always built from its own connection's id and an
  id never changes. The export still writes the bundle: whoever is in this
  state is exactly who needs a backup of it. It now names the entry, the
  slot, and the connection the slot belongs to, on the machine where the
  problem can actually be fixed.
- **The MCP server says which build is answering** — `get_server_info`
  ([ADR-0116](docs/decisions.md)), and the same version now opens the
  handshake instructions. `dbboard-mcp` is a file someone copied into
  place and it never updates itself, so an installed copy can sit several
  releases behind the behaviour expected of it — a bug reported against
  the MySQL path had already been fixed a release earlier, and neither the
  agent nor the operator could tell. The version goes on two channels
  because neither alone is dependable: the instructions arrive without
  anyone asking but some clients drop them, and a tool result always
  arrives but only if something thinks to call it. It reports the build
  and nothing else — no config path, because on Windows that path holds
  the operator's OS username and a tool result is written to the calling
  agent's transcript in plain text.

### Changed

- **The update dialog says what changed** ([ADR-0115](docs/decisions.md)).
  It used to read "dbboard v0.9.0. See the release page for the full
  changelog." — true, and no help to someone deciding whether to restart
  now. It now shows this file's summary of the version being offered: the
  paragraph under the version heading, or the entry titles where a version
  has none. The dialog is the only moment anyone is asked to decide
  something about a release, and until now it was the one place in the
  project where "what changed?" was answered with a pointer.

### Fixed

- **A refused import is no longer reported as "already present"**
  ([ADR-0112](docs/decisions.md)). The import report had one list for
  everything it did not take, and three different conditions went into
  it: the id was listed twice in the file, the id already existed and
  overwrite was off, or the entry named a saved-secret slot belonging to
  a different connection. Only the middle one means "already present",
  and only the middle one is resolved by re-importing with overwrite on
  — so the last one, which is a deliberate refusal to overwrite a live
  credential, was described as a routine skip and followed by a hint
  that could not change the outcome. The three are now reported apart,
  and a refusal names both sides of the collision: which slot the entry
  wanted and which connection holds it. The check itself is unchanged;
  nothing is imported that was not imported before.

### Security

- **The dependency advisory check runs in CI**
  ([ADR-0117](docs/decisions.md)). `cargo deny check` has been configured
  in this repo for a long time and named in `CLAUDE.md` as part of the
  security posture, but no workflow ran it, so nobody found out it was
  red: 21 advisories and four license failures, surfaced only because a
  pre-release review ran it by hand. It is now a `deps` job on every push
  and pull request to `develop` and `main`.
- **`h2` moved to 0.4.17**, past the 0.4.16 that patches
  [RUSTSEC-2026-0258](https://rustsec.org/advisories/RUSTSEC-2026-0258).
- **Known exposure, stated rather than suppressed.** Turning on libSQL's
  remote transport (above) brings `hyper-rustls` 0.25 with it, and that
  pins `rustls-webpki` 0.102 and `h2` 0.3 — versions whose fixes landed
  on major lines they cannot reach. The rest of this project already
  resolves current copies of both. Every advisory that cannot be fixed
  today is listed individually in `deny.toml` with its own reason, and
  the reasons say which are unreachable here, which are reachable and
  narrow, and which clear as soon as libSQL moves up. Four of the six
  clear on that single bump.

## [0.9.0] — 2026-08-19

Seven MCP verbs, and what they are for. Checking a change by hand means
switching the language, typing the SQL, pressing Run, and then looking —
and only the last of those is a judgement. This release moves the other
three off the person's hands and leaves the looking where it belongs. The
one fix that is not documentation came out of exactly that: the query
toolbar had been frozen since some earlier release, and nobody had reported
it.

### Added

- **The UI language can be read and set over MCP** — `get_ui_locale` and
  `set_ui_locale` ([ADR-0107](docs/decisions.md)). The chosen language now
  lives in `ui-settings.toml` instead of only in the webview's
  `localStorage`, and a running window picks up a change within about a
  second without restarting. `get_ui_locale` returns the codes this build
  ships alongside the current one, so an agent has something to pick from;
  matching is exact, and `ja-JP` is refused where `ja` is accepted. They
  exist because verifying eleven locales means switching the language
  eleven times, and the switching — unlike the judging — is mechanical.

- **An agent can see the window it is being asked about** — `capture_window`
  ([ADR-0108](docs/decisions.md)) returns a PNG of the running dbboard
  window, so "did the language change?", "is the grid full of boxes?" and
  "is this error legible?" stop being assertions and become observations.
  It reads no database. The window is found by application name rather than
  title, because a terminal tab called "dbboard" enumerates with exactly
  that title and capturing it would look like a success. Being minimised or
  closed is reported as something only a human can fix, not as a retryable
  failure. The image is the operator's real screen, so the tool says on its
  own surface that it must not be pasted anywhere public without asking.

- **An agent can now work that window, not only watch it** —
  `set_editor_sql`, `run_query`, `open_ai_panel` and `open_ai_settings`
  ([ADR-0109](docs/decisions.md)) put SQL in the query editor, press Run,
  open the AI panel, and open the provider settings inside it. With `capture_window` they close the loop: the
  mechanical half of a UI check — set it up, perform it, photograph the
  result — no longer needs the person's hands, which is what had made the
  remaining CJK rendering checks expensive enough to skip. The instruction
  crosses through two files in the config directory, one written by each
  side, numbered so that "run it again" is a second run rather than a
  no-op, and answered when the window has *finished* rather than when it
  started — a tool that returned early would report the previous run's
  outcome as this one's. A command left behind by a session that ended is
  adopted at startup instead of obeyed. `run_query` is not a cheaper
  `run_read_query`: it runs whatever the editor holds, against the
  connection that window has selected, and leaves the rows on someone's
  screen. All four fail outright when dbboard is not running, and say so
  in those words, because an agent told "timed out" retries.
  `open_ai_settings` is refused for a second reason — the AI panel being
  shut — and that refusal is the useful half of it: there is no top-level
  route to the provider settings, so being told the verb has no owner is
  how the arrangement can be checked rather than taken on trust.

### Fixed

- **The query editor's toolbar stopped responding to anything once a
  connection was selected.** The history count stayed at `(0)` however many
  queries had run, the Run button kept whichever enabled state it had at
  startup, and a language change left that one strip of the window in the
  previous language while the rest of the app switched. One cause: reading
  the per-connection history cached its first disk read into reactive
  state, and Svelte forbids writing reactive state while a `$derived` is
  being evaluated — the resulting error killed the effect that renders the
  toolbar, freezing every binding in it at its last good value. Reading
  history is now a read.

- **The HTTP contract described a payload that stopped being accurate.**
  `docs/api-contract.md` is the public API for SemVer purposes
  ([ADR-0011](docs/decisions.md)), and `dbboard-web` implements against it,
  so being wrong there is worse than being silent. Three things had drifted:
  the `id` list named three adapters when nine ship (`mysql`, `neon`,
  `supabase`, `aurora-dsql`, `firestore`, `mongodb` were all missing),
  `has_foreign_keys` ([ADR-0054](docs/decisions.md)) had been serialized for
  months without appearing in the document, and the `GET /capabilities`
  example disagreed with the `Capabilities` section in the same file.
  Passages describing every flag as `false` "in Phase 2" were rewritten to
  say what is true now. Two tests
  (`crates/dbboard-connect/tests/api_contract_drift.rs`) read the document
  and fail when either set drifts again; the capability check derives its
  list from a serialized `Capabilities`, so a new flag is covered with no
  test edit.
- **The contract now says what a capability flag does and does not
  promise.** It previously read as though every `true` flag came with an
  HTTP endpoint defined in the same document. Several describe capabilities
  the desktop client reaches over Tauri IPC ([ADR-0089](docs/decisions.md)),
  and their endpoints are specified as they land — additively, so a client
  seeing a flag with no endpoint is looking at unfinished surface, not drift.

## [0.8.0] — 2026-08-14

The first release cut from *using* the previous one. No new adapter: every
change here is something that got in the way while running dbboard against
real databases.

### Added

- **A document cell opens as a tree** ([ADR-0100](docs/decisions.md)). A
  Firestore or MongoDB document arrived in the grid as one truncated line of
  JSON — every value present, none of them findable. The cell keeps that
  one-line preview and now opens as an indented, collapsible tree in the
  read-only value dialog. A collapsed container stays visible showing its size
  (`{3}`, `[2]`) rather than vanishing with its children, and those previews
  are digits rather than words so they read the same in all 11 locales.
  Copying still yields the document itself, now pretty-printed.
- **A status bar**, carrying the two things that were not already on screen
  ([ADR-0101](docs/decisions.md)): how long the last statement took, which the
  app measured nowhere, and the running version. Connection health and the row
  count are deliberately absent — both are already two centimetres away, and
  repeating them is the filler this was asked not to be. An available update
  now also survives dismissal: closing the notice used to discard it for the
  rest of the session, and a chip in the bar brings it back.
- **An ENUM column is edited by picking a member**
  ([ADR-0102](docs/decisions.md)). Inline editing gave every column the same
  text box, which turned a closed set of values into a spelling test whose
  failure surfaced only when the `UPDATE` came back rejected — or, on a lax
  server, after a truncated value had already landed. The members come from
  the schema read the browse already performs. A value outside the declared
  members is kept and selected rather than rewritten by the act of opening the
  editor; a declaration that cannot be parsed yields no list at all and the
  column stays free text, because an editor that can only write a wrong value
  is not a safer editor. MySQL only today, and `SET` keeps its text box.
- **`aurora-dsql-iam` connections are added and edited in the app**
  ([ADR-0103](docs/decisions.md)). The kind was config-file-only: the manager
  showed a disabled Edit button and the path to `connections.toml`. That is
  not an answer for the deployment this matters for, where the person who has
  to rotate an AWS key pair is not the maintainer. The keyring field name is
  pinned, so refs written by hand before this change keep resolving. Only the
  secret access key is masked — the access key id stays legible, because the
  operator has to see which pair they are rotating away from.
- **Export takes a selection; import can overwrite**
  ([ADR-0105](docs/decisions.md)). The bundle was all-or-nothing in both
  directions, which made the two ordinary jobs impossible: handing one
  connection to one person, and refreshing a connection someone already has
  after its credentials rotated. Export now takes an explicit list, and an
  empty one is refused rather than read as "all" — the two readings are
  opposites, and an empty bundle decrypts perfectly well and then imports
  nothing, which the recipient reads as a wrong passphrase. Import takes a
  mode, and skip stays the default: skip cannot lose a credential, overwrite
  can. The ADR-0038 keyring-collision refusal does not relax; it now tracks
  which connection owns each ref, so overwriting your own secret is allowed
  while a ref aimed at another connection's slot is still refused in both
  modes. Bundles stay readable by v0.7.0 — the file format did not change.
- **`DBBOARD_CONFIG_DIR`**, an override for the per-user config directory
  ([ADR-0097](docs/decisions.md)). Screenshotting the app meant screenshotting
  a real database host, and on Windows there was no workaround: `directories`
  reads the known-folder API rather than `%APPDATA%`, so redirecting that
  variable still opens the real profile. All five per-user files move together
  — a half-honoured override would put a demo profile's connections beside the
  real profile's query history.

### Fixed

- **A pasted leading space no longer breaks a connection**
  ([ADR-0099](docs/decisions.md)). One stray space in a base URL failed at
  request construction, so the app reported "builder error" while the form on
  screen showed correct values. Non-secret text is now trimmed on both add and
  edit; secrets stay verbatim, where a leading space may be part of the value.
  The Firestore adapter also trims and names an unusable base URL, so a
  connection already saved with stray whitespace works again without being
  re-entered.
- **The export dialog's connection list is legible again.** Every checkbox was
  being stretched to the full width of its field, which pushed each name to
  the right and wrapped it, leaving no two rows starting at the same edge.

### Changed

- **CI runs the mandatory verification commands**, rather than trusting that a
  git hook ran on the contributor's machine. Until now the only checks on a
  pull request were the PII scan and the tag-triggered release build, so a
  branch could prove it leaked no PII and nothing about whether it compiled.
  Three jobs now gate every pull request: `cargo fmt --check` / `clippy -D
  warnings` / `check` / `test` across the workspace, `svelte-check` and vitest
  from `apps/desktop`, and the download site's tests. They run on Linux —
  a Windows job would be permanently red on green code (#131), and a check
  nobody can trust trains people to merge past red.

## [0.7.0] — 2026-08-11

### Added

- **`dbboard-mongodb`**, the second non-SQL adapter
  ([ADR-0095](docs/decisions.md), [ADR-0096](docs/decisions.md), issue
  0020). `ping`, `list_tables`, `query` and `describe_table` against
  MongoDB through the official driver. The query text is a command document
  as JSON — `{"find": "users", "limit": 10}` — which is MongoDB's own native
  form, with no translation layer, and results come back as ordinary rows
  with nested documents in `Value::Json` cells. `describe_table` samples a
  collection and reports each field with the sample it was inferred from
  (`string (12/20 sampled)`), because a collection declares no schema.

  Unlike Firestore, read-only here cannot be structural: every MongoDB
  command travels the same `runCommand` path, so the guarantee is a
  classifier. It allowlists both the commands and, per command, their
  *options* — so `{"find": …, "insert": …}` is refused because `insert` is
  not a `find` option, rather than because someone remembered it writes. It
  walks the whole document for `$out` and `$merge` at any depth, refuses
  server-side JavaScript deliberately, and never quotes the caller's command
  back in a refusal. Verified end-to-end against MongoDB 8, including that a
  refused write leaves the collection untouched.
- **MongoDB is selectable in the desktop client** (issue 0020). The whole
  connection URI is one masked field rather than the five DSN boxes,
  because the password rides in its authority — it goes to the OS keychain
  like every other secret and is never read back. *Database* may be left
  blank when the URI's path already names one. The sidebar generates a
  bounded `{"find": …}` command instead of `SELECT * … LIMIT`, and keeps
  *Count rows*, which MongoDB answers through a command the read allowlist
  already permits.
- **MongoDB connections are usable from the MCP server** (issue 0020). The
  tool descriptions say that `mongodb` takes a command document rather than
  SQL, with an example, so an agent told only "SQL" does not send a
  `SELECT` and read the parse error as its own mistake.

### Changed

- **MongoDB connections refuse an SSH tunnel** ([ADR-0096](docs/decisions.md)).
  One URI may name several hosts and `mongodb+srv://` discovers a whole
  replica set from DNS, so rewriting a single host to a loopback forward
  leaves the driver failing over to members the tunnel never covered — it
  appears to work, then silently stops. Refusing at configuration time is
  the honest failure.

## [0.6.0] — 2026-08-10

The first database that is not a SQL database. A Cloud Firestore
collection is now listable, queryable and describable from the desktop
client and from the MCP server, and the row model grew a nested cell so a
document has somewhere to land.

### Added

- `Value::Json`, a cell variant carrying a nested document tree, encoded on
  the wire as `{ "$json": … }` ([ADR-0091](docs/decisions.md), issue 0018).
  The prerequisite for the Firestore and MongoDB adapters: a document is a
  tree, and the flat variants had nowhere to put one. The payload is opaque
  — a document containing a `"$blob"` key stays that document — and
  `{ "$json": null }` is a document holding JSON null, not a SQL `NULL`.
  Purely additive: no existing adapter emits it, and SQL `JSON`/`JSONB`
  columns still arrive as `Text`. Consumers of the HTTP contract must accept
  the tag; see the `$json` section of [`docs/api-contract.md`](docs/api-contract.md).
- **`dbboard-firestore`**, the first non-SQL adapter
  ([ADR-0093](docs/decisions.md), issue 0019). `ping`, `list_tables`,
  `query` and `describe_table` against Cloud Firestore over its REST API,
  with service-account or emulator credentials. The query text is a
  Firestore `StructuredQuery` in JSON — the same object Google's docs show,
  with no translation layer — and results come back as ordinary rows, with
  nested documents in the new `Value::Json` cells. `describe_table` samples
  a collection and reports each field with the sample it was inferred from
  (`string (12/20 sampled)`), so an inference never renders as a declared
  schema. Read-only is structural rather than parsed: the crate contains no
  code able to build a Firestore write URL. Verified end-to-end against the
  local Firestore emulator, through the same `connect_adapter` path the
  client and the MCP server use, and with the browse query asserted as the
  exact text the sidebar generates.
- **Cloud Firestore is selectable in the desktop client**
  ([ADR-0094](docs/decisions.md), issue 0019). It is the first connection
  whose credential is optional: leaving the service-account box blank and
  ticking *Connect to the local emulator* is a valid configuration, not an
  unfinished form, and the edit form reopens in whichever of the two states
  the connection is actually in. The service-account JSON is stored in the
  OS keychain like every other secret and is never read back. A blank
  *Database ID* means the project's `(default)` database. The sidebar
  generates a bounded `StructuredQuery` instead of `SELECT * … LIMIT`, and
  drops *Count rows*, which Firestore answers through an endpoint this
  adapter does not implement; a browsed collection is read-only in the grid.
  Configurable without the UI through `DBBOARD_FIRESTORE_PROJECT_ID` and
  friends.
- **Firestore connections are usable from the MCP server** (issue 0019). No
  new wiring — every tool already went through the adapter trait — but the
  tool descriptions now say which kinds exist and that `firestore` takes a
  `StructuredQuery` rather than SQL, with an example. An agent that is only
  told "SQL" sends a `SELECT`, gets a parse error, and reads it as its own
  mistake. `list_connections` now names every kind the server can return,
  guarded by a test against the source rather than a hand-kept list: that
  list had already gone stale twice.
### Fixed

- Documentation described a write policy the code does not have. `run_write`
  never accepted `DROP INDEX` or `COMMENT ON`, and `DROP` is permanently
  closed for *every* object including an index — the allowlist is
  `INSERT` / `UPDATE` / `DELETE` / `MERGE` plus `CREATE TABLE` / `VIEW` /
  `INDEX` / `SCHEMA` and `ALTER TABLE`, exactly as ADR-0087 decided. The
  README, `docs/connections.md` and the 0.5.0 entry below said otherwise;
  two tests now pin the behaviour so the wording cannot drift again.
- `crates/dbboard-mcp/README.md` documented handing the MCP server a whole
  connection through `DBBOARD_MYSQL_URL` and friends. Those variables belong
  to `dbboard-server`'s single-connection resolution path; the MCP server
  resolves a tool call's `connection_id` against `connections.toml` and the
  keychain and reads none of them. It honours `--config` / `DBBOARD_CONFIG`,
  which choose *which* store file, and that is now what the section says.
- `docs/compatibility.md` had no MySQL / MariaDB section at all, three
  releases after the adapter shipped (ADR-0068).

## [0.5.1] — 2026-08-06

A patch release with nothing in it but two bugs found by using the thing.
Both broke a real workflow rather than an edge case: a connection through
an SSH bastion would die and stay dead until the app was restarted, and
schema introspection failed against every MySQL 8 table — which also meant
the MCP tool `list_relationships` had been returning an empty result
instead of an error.

No contract change; [`docs/api-contract.md`](docs/api-contract.md) is
unchanged from 0.2.0.

### Fixed

- **A connection through an SSH bastion could die and stay dead until the
  app was restarted** ([ADR-0092](docs/decisions.md)). Every call after it
  failed with `expected to read 4 bytes, got 0 bytes at EOF`. The tunnel now
  sends SSH keepalives (russh sends none by default, so an idle session was
  simply reaped), and a cached connection that has been idle for 30 seconds
  is pinged before it is used again — a failed ping throws it away and dials
  a fresh one, which is what rebuilds the forward.

- MySQL schema introspection failed on every table against MySQL 8, with
  `type conversion failed: … Rust type alloc::string::String (as SQL type
  VARCHAR) is not compatible with SQL type BLOB`. Since 8.0 the
  `information_schema` views are served from the data dictionary, which
  declares `TABLE_NAME` as `VARBINARY` and `DATA_TYPE` as `BLOB`, so
  sqlx's type check rejected bytes that were perfectly good UTF-8. Metadata
  text is now read as bytes and validated here, the way query cells already
  were. This unbreaks `describe_table`, `table_ddl`, `foreign_keys` and —
  through them — the MCP tools `describe_table`, `search_schema` and
  `list_relationships`, the last of which had been failing *quietly*: it
  swallows a per-table introspection error, so it returned an empty set of
  relationships instead of an error.

### Added

- **A reconnect action**: a reload icon on the connection pill, and a
  Reconnect button in the error banner. The app now heals a dead connection
  on its own, so this is for recovering *now* rather than on the next call.

## [0.5.0] — 2026-08-05

Fifth tagged release, and the one that makes dbboard usable by something
other than a person. `dbboard-mcp` gains the ability to **write** behind
a per-connection gate and a fail-closed allowlist, an **alias** that
keeps real connection ids out of an agent's transcript, and — the part
that was silently missing — **a way to be obtained at all**: the MCP
server is now published as a release binary rather than existing only as
a `cargo build` invocation.

This release also **retires the egui client**. Tauri 2 + SvelteKit is
the only client from here. Installs of the egui build stay on v0.4.0,
which is still downloadable and still works; it has no updater, so
moving off it means downloading the app from the
[download page](https://meta-taro.github.io/dbboard/).

The HTTP contract in [`docs/api-contract.md`](docs/api-contract.md) is
unchanged from 0.2.0.

### Added

- `dbboard-mcp` can **write** — `run_write` runs `INSERT` / `UPDATE` /
  `DELETE` / `MERGE` and `CREATE TABLE` / `VIEW` / `INDEX` / `SCHEMA` /
  `ALTER TABLE`
  behind a three-tier policy (ADR-0087): the connection must be opted in
  with `mcp_write = true`, the statement must parse to something on the
  allowlist, and a permanently-closed list — `GRANT` / `REVOKE` / `DENY`,
  user and role DDL, `SET PASSWORD`, `TRUNCATE`, and `DROP` of anything
  at all — is refused whatever the flag says. Classification is on
  the AST and fails closed. Every existing connection stays read-only
  across the upgrade, because an absent flag means `false`.
- `dbboard-mcp` gained `dump_database`, so an agent can take a backup
  before it writes. It never overwrites an existing file.
- **`dbboard-mcp` is a downloadable binary** (ADR-0090). Releases now
  carry `dbboard-mcp-windows-x86_64.exe` and
  `dbboard-mcp-macos-universal` with checksums, from the same tag as the
  app. Until now the only way to get the MCP server was to build it, so
  the documented `claude mcp add dbboard -- /absolute/path/to/dbboard-mcp`
  named a file that could not exist without a Rust toolchain — an AI
  agent told to use it searched, found nothing installable, and gave up.
  The download page still offers only the desktop app; the MCP server is
  a separate download from the release page, and the setup docs now give
  a concrete install path and the exact `claude mcp add` line per OS.
- `dbboard-mcp` documents passing credentials as environment variables
  (`DBBOARD_*` in the agent's `env` block) for agents that operate under
  a rule against writing credentials to a file, and states plainly that
  TLS uses the OS trust store with no flag to pass behind a
  TLS-terminating corporate proxy (ADR-0034).
- Connections carry an `mcp_write` flag, settable in `connections.toml`
  or from the app (*Connections → Edit → AI agent access*). Editing a
  connection without touching the toggle keeps whatever is stored, so a
  rename cannot silently revoke the permission.
- Connections carry an optional `mcp_alias` — the name `dbboard-mcp`
  shows an AI agent **instead of** the connection's id *and* name
  (ADR-0088). Ids and names are whatever you typed, so on a real install
  they leak a host or a store into the agent's transcript and its
  provider's logs. With an alias set, that is the only string the agent
  sees, and the real id stops working as a handle — one learned from an
  older session cannot be handed back. Aliases must be unique across
  every alias and id. Absent by default and settable in the same place
  as the write gate; everything below the MCP boundary
  (`annotations.toml`, `DBBOARD_CONNECTION`, the connection list) keeps
  using the real id.

### Changed

- **The Tauri 2 + SvelteKit app is the only dbboard client** (ADR-0089).
  It reached parity at v0.4.0 and has since led the egui client — the
  connection form edits SSH tunnels, which egui never learned. Building
  every write surface twice was costing more than the second build
  returned.
- The download page picks builds by product name (`dbboard-desktop…`)
  rather than by file extension, so a release that still carries both
  clients' assets — v0.4.0 does — cannot offer the retired one. This
  supersedes #135, where the answer depended on the order the GitHub
  Releases API happened to return assets in.
- Every generated release page now opens with a link to the
  [download page](https://meta-taro.github.io/dbboard/). A release lists
  raw asset filenames; someone arriving from a search result should not
  have to work out which one is theirs.

### Removed

- The egui client: `crates/dbboard-ui`, `apps/dbboard`, and the
  `eframe` / `egui_extras` / `egui_commonmark` dependencies. Releases up
  to and including v0.4.0 still carry its binaries and keep working;
  nothing after v0.4.0 ships them. It has no updater, so an egui install
  stays on v0.4.0 until it is replaced with a download from the page
  above.
- `dbboard-windows-x86_64.exe`, `dbboard-<version>-x86_64.msi`, and
  `dbboard-macos-universal-<version>.dmg` are no longer built or
  published. `SHA256SUMS.txt` still covers everything that is.
- `crates/dbboard-i18n`, whose message catalogues were egui's; the Tauri
  client carries its own under `apps/desktop/src/lib/i18n/`.
  `crates/dbboard-server` is **kept** despite losing its last in-repo
  consumer — it is the executable statement of the HTTP contract
  `dbboard-web` mirrors ([`docs/api-contract.md`](docs/api-contract.md)).

### Fixed

- The Tauri release jobs pinned Node 20 while `apps/desktop` pins pnpm 11,
  which imports `node:sqlite` (Node 22.5+); `pnpm install` died before
  resolving a package. Both jobs now run Node 22.

### Documentation

- `dbboard-mcp`: document registering the server with **Claude Code**
  (`claude mcp add`), which the top-level README claimed was covered but
  was not; correct the tool count in the README (five → seven); and add
  sections on restarting after a config change, running several agents at
  once against a **tunneled** connection, and reading connection-failure
  symptoms.
- Download page: mention the MCP server, and list MySQL/MariaDB among the
  supported engines.
- **dbboard is easier to find.** Publishing it was not the same as making
  it findable: an agent searching the web for dbboard concluded it was
  "not a publicly available tool". The repo now declares its homepage and
  topics, `README.md` leads with the download link, the site carries
  canonical / Open Graph tags plus `robots.txt` and `sitemap.xml`, and
  `CLAUDE.md`, `crates/dbboard-mcp/README.md` and `apps/desktop/README.md`
  all quote the URL so anyone — human or agent — reading the repo can
  answer "where do I get it".
- The top-level docs no longer describe egui as a current client:
  `README.md`, `CLAUDE.md`, `DESIGN.md`, `docs/architecture.md`,
  `docs/api-contract.md`, `docs/compatibility.md` and `docs/roadmap.md`
  were rewritten around the Tauri client, and `docs/desktop-parity.md` is
  marked archived — it tracked a gap that no longer exists.

## [0.4.0] — 2026-08-04

Fourth tagged release, and the largest so far. Headlined by a **new
desktop client** — Tauri 2 + SvelteKit — which starts the release as a
read-only spike (ADR-0059) and ends it at feature parity with the egui
build, then overtakes it with SSH-tunnel editing. Also lands **MySQL /
MariaDB** as the fourth engine and the first genuinely different SQL
dialect, logical **dump and restore**, an **OpenAI** provider, and
automated **PII / secret leak scanning** on every commit.

Both clients ship from the same tag; the egui single-exe build is still
released and still works. The HTTP contract in
[`docs/api-contract.md`](docs/api-contract.md) is unchanged from 0.2.0.

### Added

- **A second desktop client (Tauri 2 + SvelteKit)**, now the primary one.
  It began as an explicitly read-only spike over the egui-free core
  (ADR-0059) — frameless titlebar, sidebar shell, Query/Structure tabs, a
  design-token system with Auto/Light/Dark, a CodeMirror 6 SQL editor
  (ADR-0060), and a result grid with sort, selection, export, and a cell
  popup. Parity with egui then arrived slice by slice:
  - **Connection management write path** (ADR-0062) — create, edit,
    delete, plus passphrase-encrypted bundle import/export. Credentials
    are entered as **parts, never as a hand-written DSN** (ADR-0073); TLS
    is an explicit form choice defaulting to required, with no silent
    plaintext fallback (ADR-0078, ADR-0079); every filesystem path has a
    Browse button (ADR-0077); kinds that only exist in `connections.toml`
    are disabled in the list rather than refused on submit (ADR-0074);
    the edit form asks for exactly the fields the add form does
    (ADR-0080).
  - **Inline cell editing** (ADR-0063) — the first DB-write surface in
    the Tauri app, with an explicit save step.
  - **Logical backup and restore** (ADR-0064, ADR-0065) wired to the same
    core orchestrators the egui build uses.
  - **AI assistant** (ADR-0066) over the existing provider layer.
  - **Auto-update** (ADR-0067) — `tauri-plugin-updater` against a
    `latest.json` manifest assembled by release CI and verified with a
    signing key held only as a repository secret.
- **MySQL / MariaDB adapter** (ADR-0068) — the fourth engine, and the
  first that is not Postgres- or SQLite-shaped. Row-producing paths pin
  the simple/text wire protocol (ADR-0070); generated SQL follows the
  connection's identifier dialect (ADR-0072); the statement-timeout
  variable is **probed rather than assumed**, since MySQL spells it
  `max_execution_time` and MariaDB `max_statement_time` (ADR-0081); a
  listed table nobody can read degrades instead of failing the whole
  sweep (ADR-0071).
- **SSH tunnel** (ADR-0069) — pure-Rust local port forwarding (russh) for
  databases that only listen on a bastion's `localhost`. Host-key
  verification is **mandatory** (fingerprint pin XOR `known_hosts`, never
  blind trust); the connection form can fetch the host key for you to
  confirm instead of demanding you paste it (ADR-0076). Editing lives in
  the Tauri app only — egui users still hand-edit `connections.toml`.
  Passphrases and SSH passwords go to the OS keychain, never to TOML.
- **Logical dump** (ADR-0049) — a plan/threshold pass, keyset paging with
  progress and cancel, SQL literal + `INSERT` rendering, `table_ddl` as
  an adapter capability with Postgres catalog reconstruction and D1
  verbatim DDL, and a user-configurable huge-DB warn threshold
  (ADR-0050).
- **Logical restore / import** (ADR-0051) — a SQL statement splitter, a
  `sqlparser`-backed classifier, an orchestrator with an empty-target
  gate, and per-engine transaction strategy (atomic on Turso, per-
  statement on D1, transaction-or-fallback on Postgres/DSQL).
- **OpenAI (ChatGPT) provider** (ADR-0052) — a `dbboard-openai` Chat
  Completions provider wired through config, admin, UI, and i18n.
- **Two more read-only MCP tools**: `search_schema` for name lookup
  across a connection (ADR-0053) and `list_relationships` for
  foreign-key introspection (ADR-0054), bringing the tool set to seven.
- **PII / secret leak scanning** (ADR-0055) — `scripts/pii-scan.sh` runs
  on every commit, every commit message, and daily in CI, blocking real
  store names, credentials, and maintainer PII from entering a public
  repo. Commit identity is scanned too, because it cannot be edited
  after the fact (ADR-0084).
- **A branded egui design system** (ADR-0056) — palette, tokens, theme
  module — applied to the primary CTA, header identity, and count badge
  (ADR-0057).
- **A download page on GitHub Pages** (ADR-0047), plus Start Menu and
  Desktop shortcuts in the MSI.
- **Sortable result grid**, up to three columns (ADR-0048).

### Changed

- Aurora DSQL no longer receives the read-only transaction preamble; it
  rejects it, so the guard is applied differently there (ADR-0061).
- Long cell values open an editor instead of being read through a
  keyhole (ADR-0082); the sidebar splits and popovers place themselves
  (ADR-0083).
- The desktop lib is `rlib`-only, so its fingerprint can vary per build
  configuration — `cargo build` and `cargo test` no longer evict each
  other's fingerprint and recompile it. Pre-push dropped from ~136s to
  ~58s (ADR-0086).

### Fixed

- Long error messages wrap in the inline error banner instead of
  overflowing.
- The header pill and theme toggle moved off the menu bar, where they
  overlapped (ADR-0057).
- The PII identity allowlist admits GitHub's `web-flow` committer, so
  commits authored through the GitHub web UI no longer fail the scan
  (ADR-0085).

### Security

- SSH host-key verification is mandatory and has no "trust anything"
  path (ADR-0069).
- SSH key passphrases and passwords are stored only in the OS keychain;
  `connections.toml` holds a reference, never a secret. Switching auth
  method no longer persists a keyring reference that was never written.
- PII / secret scanning is wired into the commit path and CI (ADR-0055),
  with a known gap recorded: without a local `.pii-denylist` the
  business-identifying rules cannot fire, so the untracked denylist and
  the `PII_DENYLIST` CI secret are load-bearing.

## [0.3.0] — 2026-07-21

Third tagged release. Headlined by **dbboard as a read-only MCP server**
(ADR-0046) so an external AI agent — Claude Desktop, Claude Code — can
drive the databases dbboard is already configured with, without ever
seeing a secret and without being able to write. Also rolls up local
column/table annotations, the signed-off distribution installers +
Release CI, and a batch of AI-panel and packaging fixes. Desktop-only;
the HTTP contract in [`docs/api-contract.md`](docs/api-contract.md) is
unchanged from 0.2.0.

### Added

- **Read-only MCP server** (`dbboard-mcp`): a standalone stdio binary
  that exposes dbboard's configured connections to an MCP client as five
  read-only tools — `list_connections`, `list_tables`, `describe_table`,
  `run_read_query`, `get_annotations`. Read-only is **engine-enforced**
  (Postgres `BEGIN READ ONLY`, libSQL `PRAGMA query_only`, D1 AST
  classification), not string matching; results are bounded; secrets stay
  in the OS keychain and are never serialized into a tool result or error.
  Connection wiring is factored into a new `dbboard-connect` crate so the
  binary reuses the server's connect path without pulling in axum
  (ADR-0046).
- **Local column and table annotations** (`annotations.toml`): an editable
  Note column on the Structure tab, keyed by connection id / table /
  column. Notes live in the config directory, never touch the database,
  work on read-only connections, and apply uniformly across every adapter
  (ADR-0045).
- **Distribution installers + Release CI** (ADR-0044): a `v*.*.*` tag now
  publishes a GitHub Release carrying Windows (`.exe` + MSI) and macOS
  (`.dmg`) artifacts with a `SHA256SUMS.txt`, via `cargo-wix` /
  `cargo-bundle`. Artifacts are unsigned for now (SmartScreen / Gatekeeper
  warnings remain; code signing is tracked separately).

### Changed

- **AI scope caption** in the assistant panel now reads as a standing,
  emphasised guarantee — the assistant only drafts SQL, it never runs it
  or touches data on its own — instead of dismissible fine print
  (ADR-0045 follow-up).
- **Default Anthropic model** bumped to `claude-sonnet-5`.
- **Help menu** renders update-notice release notes as Markdown and stays
  open on inside clicks so links and change notes are usable (ADR-0043).

### Fixed

- **Anthropic streaming errors** now surface the API response body (e.g.
  insufficient balance, invalid model) instead of a bare `status 400`.

### Security

- `cargo deny` advisory/license drift resolved: three transitive
  build-time advisories (proc-macro-error2 unmaintained, the quick-xml
  DoS pair) documented as ignores with reasons, `MPL-2.0` allowed for
  `option-ext`, and the dead `CDLA-Permissive-2.0` allowance trimmed.
- A `security-reviewer` pass over the MCP crate found no CRITICAL/HIGH
  issues; the five secret/read-only invariants are verified at the source.

### Documentation

- ADR-0043 through ADR-0046 capture the decisions since 0.2.0.
- `crates/dbboard-mcp/README.md` (tool table, security posture, Claude
  Desktop wiring) and the dbboard-connect / dbboard-mcp entries in
  [`docs/architecture.md`](docs/architecture.md).

## [0.2.0] — 2026-07-17

Second tagged release. Rolls up Phase 3 (multi-connection management),
Phase 4 (AI assistant), the Windows internal-distribution work, and the
in-use quality-of-life batch. Desktop-only; the HTTP contract in
[`docs/api-contract.md`](docs/api-contract.md) is unchanged from 0.1.0.

### Added

- **AI assistant** (`dbboard-ai` + Anthropic provider): natural-language
  → SQL with streaming output, cooperative cancel, a token meter, and
  schema-aware prompting via full `describe_table` DDL (ADR-0023 through
  ADR-0028).
- **Inline cell editing with explicit Save** (HeidiSQL-style): double-click
  a cell to edit, blur stages it, a pinned Save row commits every staged
  edit via a primary-key `UPDATE`. Editable only for single-table browse
  results with a resolved primary key (ADR-0042).
- **Multiple named connections** with OS-keychain secrets, live switching,
  and **encrypted `.dbbx` bundle export/import** (passphrase-encrypted,
  carries connections + resolved secrets in one file; ADR-0038).
- **Aurora DSQL** support with self-minted SigV4 IAM auth and timer-based
  token pool-swap so long-lived sessions don't get recycled
  (ADR-0036 / ADR-0037).
- **Query workflow**: persisted history, a Structure tab, an auto-`LIMIT`
  guard for bare `SELECT`s, result export (CSV / JSON), expandable cells,
  and right-click table quick-SQL that runs on pick (ADR-0030 / ADR-0031 /
  ADR-0035).
- **Light / Dark / Auto theme** that follows the OS setting, persists the
  choice, and syncs the Windows title bar (ADR-0041).
- **Startup update check** against GitHub Releases: a non-blocking,
  opt-out (`DBBOARD_NO_UPDATE_CHECK`) notification in the Help menu when a
  newer version is published (ADR-0040).
- **Unified error surface**: copyable, bilingual (Japanese + original
  English) error display (ADR-0039).
- **Localisation** across 11 locales.
- **Windows packaging**: console-suppressed release binary with embedded
  icon and version metadata, statically linked CRT (no VC++ redist), and
  in-tree `cargo-wix` MSI sources (ADR-0032).

### Documentation

- ADR-0012 through ADR-0042 capture every non-trivial decision since 0.1.0.
- Maintainer runbooks and tester onboarding for the internal distribution
  under [`docs/maintainer/`](docs/maintainer/) and
  [`docs/internal-testing.md`](docs/internal-testing.md).

## [0.1.0] — 2026-05-25

First tagged release. Closes Phase 1 (Turso vertical slice) and the
follow-on Phase 1.5 / 1.6 / 1.7 work; see
[`docs/roadmap.md`](docs/roadmap.md).

### Added

- **Database adapters** for the initial scope:
  - `dbboard-turso` — Turso / libSQL (`:memory:` and local file).
  - `dbboard-d1` — Cloudflare D1 via REST `/raw` (Phase 1.6, ADR-0007).
  - `dbboard-postgres` — PostgreSQL-wire (CockroachDB and Neon use the
    same adapter; Phase 1.7, ADR-0008).
- **Local HTTP backend** `dbboard-server` (axum) bound to loopback on
  an OS-assigned port; UI is now an HTTP client (Phase 1.5,
  ADR-0006 / ADR-0009).
- **egui UI** with table sidebar, SQL editor, result grid, and inline
  error surface.
- **HTTP contract** in [`docs/api-contract.md`](docs/api-contract.md) —
  the canonical surface shared with `dbboard-web`.
- **10,000-row cap** per query, uniform across adapters, returned as a
  `query` error (HTTP 400) instead of silently truncating.
- **Versioning & DB-support policy**: SemVer with the HTTP contract as
  the public API; tiered backend support
  ([ADR-0011](docs/decisions.md), [`docs/compatibility.md`](docs/compatibility.md)).
- **`cargo-deny`** configuration gating the dependency graph on
  advisories, licenses, duplicates, and unknown sources.
- **`cargo-husky`** pre-commit and pre-push hooks running fmt, clippy
  (`-D warnings`), check, and tests; pre-push additionally runs release
  build and tests, skipping on deletion-only pushes.

### Security

- TLS hardening for the Postgres adapter: `sslmode=Prefer` is upgraded
  to `Require` (explicit `disable` is respected) to avoid silent
  plaintext fallback.
- D1 transport errors are scrubbed of URL / account ID / database ID
  before surfacing to the user.
- Turso connection errors redact the file path.
- The loopback server is unauthenticated by design; widening the bind
  or persisting the port requires a per-launch secret first (ADR-0009).

### Documentation

- ADR-0001 through ADR-0011 capture every non-trivial decision so far.
- README, `docs/architecture.md`, `docs/api-contract.md`,
  `docs/compatibility.md`, and `docs/roadmap.md` reflect the shipped
  scope.

[Unreleased]: https://github.com/meta-taro/dbboard/compare/v0.10.0...HEAD
[0.10.0]: https://github.com/meta-taro/dbboard/compare/v0.9.0...v0.10.0
[0.9.0]: https://github.com/meta-taro/dbboard/compare/v0.8.0...v0.9.0
[0.8.0]: https://github.com/meta-taro/dbboard/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/meta-taro/dbboard/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/meta-taro/dbboard/compare/v0.5.1...v0.6.0
[0.5.1]: https://github.com/meta-taro/dbboard/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/meta-taro/dbboard/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/meta-taro/dbboard/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/meta-taro/dbboard/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/meta-taro/dbboard/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/meta-taro/dbboard/releases/tag/v0.1.0
