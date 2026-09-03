# Taking screenshots of dbboard

Screenshots of a database client are a leak surface. The window shows a
connection list, a schema, and rows of somebody's data all at once, and the
parts that matter are four pixels tall. `scripts/pii-scan.sh` cannot read a
PNG, so nothing downstream will catch what the capture caught — this checklist
is the whole control.

Everything published — the download page, the README, an article, a bug report
to a third party — comes from the demo profile in
[`scripts/demo-profile`](../../scripts/demo-profile/README.md). Set that up
first; the rest of this page assumes it.

## Never capture your own instance

Not "capture it and crop", not "capture it and blur". The installed instance is
the one with real hosts in it. Two instances of dbboard look identical in a
screenshot and identical in Task Manager, so check the process before you point
anything at it:

```powershell
Get-Process dbboard | Select-Object Id, Path, StartTime
```

The demo instance runs from `target/release/` and is still called
`dbboard-desktop.exe` there — that is the cargo binary, which kept its name.
Your own installed one runs from `%LOCALAPPDATA%\dbboard\` (installs made
before v0.15.0 sit in `%LOCALAPPDATA%\dbboard-desktop\`, and the installer
does not move them). Capture by process id, never by "the window in front".

## The checklist

Read the image before it goes anywhere. Every item is something that has to be
absent from the picture, not from your intent.

- **No real host, database name or connection name.** The sidebar and the
  connection badge in the top right both carry them.
- **No file path containing a username.** dbboard prints the resolved
  `connections.toml` path in its empty state, and the Windows temp directory
  contains the OS username.
- **No real store, company or person's name in the data.** Including in a
  column name, a table name, a note field, and the query text in the editor.
- **Nothing from the query history.** The history panel replays whatever the
  profile has run. A demo profile that has been used for debugging is no longer
  a demo profile.
- **No other application.** Capture the window rectangle, not the screen. A
  terminal, an editor tab or a notification behind dbboard is outside the
  control entirely.
- **No title bar of another window at the edges.** `GetWindowRect` includes the
  invisible resize border, so a capture using it picks up a few pixels of
  whatever is underneath along every side. Use
  `DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS)`.

If any of these is present, retake it. Do not edit the image: a blur that is
reversible, or a black box drawn over a layer that is still in the file, is
worse than the original because it is now believed to be safe.

## For the download page

Captures for `site/screenshots/` are English-language — the page is in English,
and a Japanese interface in the picture reads as a different product.

`node --test site/page.test.mjs` checks what can be checked mechanically: that
every image the page references exists, is same-origin (the page pins
`img-src 'self'`, so an off-origin image renders as nothing at all), and has an
alt that says what it shows. It cannot check what is inside the image. That is
the list above.

Retake all of them together when the interface changes shape, so the page never
shows two versions of the app side by side.
