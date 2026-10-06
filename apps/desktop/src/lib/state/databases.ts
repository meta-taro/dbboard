// Whether the sidebar draws a database level above the tables (ADR-0162,
// docs/every-database.md). Kept pure so the rule is tested without a store.

import type { DatabaseListing } from '$lib/api';

/**
 * A database level only for a connection saved without a database whose
 * engine can see more than one. A connection saved with a database keeps the
 * look it always had, and a single database is not worth a level of its own.
 */
export function drawsDatabaseLevel(listing: DatabaseListing | null): boolean {
  return !!listing && listing.configured === null && listing.databases.length > 1;
}
