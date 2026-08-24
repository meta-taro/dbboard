import type { ConnectionView } from '$lib/api';

// What to put in a connection row's `title`.
//
// Both places that show a connection — the sidebar list and the toolbar pill —
// used to hover their `id`. An id is generated, it is never shown anywhere
// else, and nobody has ever wanted one: the sidebar name is the thing that
// truncates, so the tooltip was hiding the only text a hover could have
// usefully revealed.
//
// The kind rides along because the pill does not show it at all, and in the
// sidebar it is small, muted and capitalised away from what the config says.
export function connectionTooltip(c: ConnectionView): string {
  // A name is required on the add form, so an empty one arrives only through
  // an import or a hand-edited TOML. The id is a poor label but an empty
  // tooltip reads as a broken one.
  const name = c.name.trim() || c.id;
  const kind = c.kind.trim();
  return kind ? `${name} — ${kind}` : name;
}
