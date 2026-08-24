import type { ConnectionView } from '$lib/api';

// Narrow the connection list by typing (issue #192, criterion 2).
//
// Name and id only. The kind is deliberately not matched: typing `my` to
// reach a connection called "my shop" would otherwise pull in every MySQL
// row as well, which is the opposite of narrowing. The id is matched
// because it is the one handle that can be pasted in from a log or an
// error message.
//
// Every whitespace-separated word has to match somewhere, so a second word
// narrows rather than widens — `shop staging` is one row, not two.
export function filterConnections<T extends Pick<ConnectionView, 'id' | 'name'>>(
  list: T[],
  query: string,
): T[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  // The same array back, not a copy: an unfiltered list must not look like a
  // new one to the `{#each}` keyed block on every keystroke.
  if (words.length === 0) return list;

  return list.filter((c) => {
    const haystack = `${c.name} ${c.id}`.toLowerCase();
    return words.every((w) => haystack.includes(w));
  });
}
