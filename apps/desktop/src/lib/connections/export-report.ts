// Turns an `ExportReport` into the sentences the connection manager shows
// after an export (issue #194). Pure and Tauri-free so the wording rules are
// unit-testable; the component only joins the result. Sibling of
// `import-report.ts`, deliberately shaped the same way.
//
// The rule this module exists to enforce: the export succeeded, and the
// warning has to read as a warning about what was written rather than as a
// failure to write it. A malformed store is exactly the store whose owner
// needs the backup most, so the first sentence is always the success.
import type { ExportReport } from '$lib/api';

type Translate = (key: string, params?: Record<string, string | number>) => string;

export function exportSummary(report: ExportReport, t: Translate): string[] {
  const lines: string[] = [t('conn-export-ok', { count: report.exported })];

  if (report.foreign_refs.length > 0) {
    lines.push(t('conn-export-foreign-lead', { count: report.foreign_refs.length }));
    for (const r of report.foreign_refs) {
      // Name both sides, as the import refusal does: told only the id, an
      // operator has nothing to look at, and told only the slot, nothing to
      // act on.
      lines.push(t('conn-export-foreign-entry', { id: r.id, ref: r.key_ref, owner: r.owner }));
    }
  }

  return lines;
}
