// A document pushed into the SQL editor from outside it — the sidebar's
// "Count rows" / "Select top 100", a history replay — held until the
// CodeMirror view exists to receive it.
//
// Why a buffer instead of letting the editor adopt a reactive `value`: adoption
// through an effect depends on when that effect runs relative to the view's
// construction, and a document that lands first is dropped with no trace. The
// symptom is the editor still showing its seed text while the result grid shows
// the answer to a query you never see. Pushing is explicit, and the ordering
// rule — buffer now, apply when the view arrives — is a unit test here rather
// than a claim about framework internals.

export class ExternalDoc {
  // `null` is "nothing pending"; `''` is a real (empty) document, which is why
  // this is not a truthiness check anywhere below.
  #pending: string | null = null;

  /** Queue `text` as the editor's next document. A push that has not been
   *  applied yet is overwritten: only the newest matters, since the one it
   *  replaces was never on screen. */
  push(text: string): void {
    this.#pending = text;
  }

  get hasPending(): boolean {
    return this.#pending !== null;
  }

  /** Take the queued document, or `null` if there is none. Taking clears the
   *  queue so an applied document is never re-applied over later typing. */
  take(): string | null {
    const text = this.#pending;
    this.#pending = null;
    return text;
  }
}
