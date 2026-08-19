// Default file names for the native "Save As" dialogs.
//
// Why this exists: every export offered a fixed name — `dbboard-connections.dbbx`,
// `dbboard-result.csv`. Exporting twice therefore proposed the same name twice,
// so the second export lands on an "overwrite?" prompt, and answering it wrongly
// destroys the first file. Even answering it correctly leaves the operator
// renaming by hand every time, which is where `connections (2).dbbx` comes from
// — a name that says nothing about which export it is.
//
// A timestamp fixes both: the dialog proposes a name that does not collide, and
// the resulting directory listing sorts chronologically and reads as a history.

const pad = (n: number, width = 2): string => String(n).padStart(width, '0');

/**
 * `YYYYMMDD-HHMMSS` for `now`, in **local** time.
 *
 * Local rather than UTC because the operator identifies a file by the clock
 * they were looking at when they made it; a UTC stamp reads as the wrong hour,
 * and either side of midnight as the wrong day.
 *
 * The layout is chosen so the name sorts chronologically under a plain
 * lexicographic sort — which is what Explorer, `ls` and the file dialog all do
 * — hence the zero padding and the big-endian field order. No `:`, because
 * Windows rejects it in a file name.
 */
export function fileTimestamp(now: Date = new Date()): string {
  const date = `${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}`;
  const time = `${pad(now.getHours())}${pad(now.getMinutes())}${pad(now.getSeconds())}`;
  return `${date}-${time}`;
}

/**
 * `<stem>-<YYYYMMDD-HHMMSS>.<ext>` — the default the save dialog opens with.
 *
 * The stamp goes before the extension rather than after the whole name so the
 * OS still recognises the type and the dialog's file-type filter still matches.
 */
export function timestampedFileName(stem: string, ext: string, now?: Date): string {
  return `${stem}-${fileTimestamp(now)}.${ext}`;
}
