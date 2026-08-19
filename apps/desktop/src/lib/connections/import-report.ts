// Turns an `ImportReport` into the sentences the connection manager shows
// after an import (ADR-0038, ADR-0105, ADR-0112). Pure and Tauri-free so the
// wording rules are unit-testable; the component only joins the result.
//
// The rule this module exists to enforce: the three not-imported reasons never
// share a sentence. "Already present" is true of exactly one of them, and the
// overwrite hint fixes exactly one of them. Merging them produced a message
// that was false for a refusal and sent the operator back through an import
// that could not behave differently the second time.
import type { ImportReport } from '$lib/api';

type Translate = (key: string, params?: Record<string, string | number>) => string;

export function importSummary(report: ImportReport, t: Translate): string[] {
  const lines: string[] = [
    t('conn-import-ok', {
      imported: report.imported.length,
      overwritten: report.overwritten.length,
      // What the bundle left behind for ordinary reasons. Refusals are
      // counted separately below: a security refusal folded into this
      // number is exactly the conflation ADR-0112 removed.
      skipped: report.skipped_existing.length + report.duplicate_in_bundle.length,
    }),
  ];

  if (report.skipped_existing.length > 0) {
    lines.push(t('conn-import-skipped-ids', { ids: report.skipped_existing.join(', ') }));
    // Name the way out. Reachable only in skip mode — the backend leaves this
    // list empty whenever overwrite was asked for — so it never suggests a
    // setting that is already on.
    lines.push(t('conn-import-skipped-hint'));
  }

  if (report.duplicate_in_bundle.length > 0) {
    lines.push(t('conn-import-duplicate-ids', { ids: report.duplicate_in_bundle.join(', ') }));
  }

  if (report.refused.length > 0) {
    lines.push(t('conn-import-refused-lead', { count: report.refused.length }));
    for (const r of report.refused) {
      lines.push(
        t('conn-import-refused-entry', { id: r.id, ref: r.key_ref, owner: r.owner }),
      );
    }
  }

  if (report.overwritten.length > 0) {
    lines.push(t('conn-import-overwritten-ids', { ids: report.overwritten.join(', ') }));
  }

  return lines;
}
