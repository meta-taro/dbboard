// Pure logic for the AI assistant UI (ADR-0052): the send-gate, the streaming
// fold, the token-meter predicate, and the provider-form model + validation.
// Kept free of Tauri so it is unit-testable in isolation; the command wrappers
// live in `$lib/api` and the wiring in the AI components.
//
// Guardrail mirrored here (the backend enforces it, this only shapes the UI):
// Explain sends only the SQL the user typed; Suggest additionally needs a
// connection so the backend can attach table/column *names*. Neither ever
// sends row data — see src-tauri/src/ai.rs.

// Wire shapes mirroring the backend DTOs (src-tauri/src/ai.rs). snake_case
// because the Rust structs derive `Serialize` with default field names.

/** The two things the assistant does. Explain needs no connection; Suggest
 *  attaches the connection's schema, so it does. */
export type AiMode = 'explain' | 'suggest';

/** The provider kinds the backend can build. Matches the `AiKindInput` tag
 *  (note: `openai`, not `open_ai`). */
export type ProviderKind = 'anthropic' | 'openai';

/** Whether the assistant is usable right now (from `ai_status`). */
export interface AiStatus {
  active: boolean;
  provider_label: string | null;
  has_streaming: boolean;
  can_manage: boolean;
}

/** One `ai:chunk` event: `text_delta` is the new text to append;
 *  `tokens_in`/`tokens_out` are the *cumulative* counts and REPLACE (never
 *  sum) the meter — matching the backend `StreamAcc` rule. */
export interface AiChunk {
  text_delta: string;
  tokens_in: number;
  tokens_out: number;
}

/** The terminal result of one explain/suggest call. */
export interface AiOutcome {
  text: string;
  tokens_in: number;
  tokens_out: number;
  cancelled: boolean;
  /** Tables whose `describe_table` failed during a Suggest with details on —
   *  non-blocking (ADR-0028). */
  prefetch_warnings: number;
}

/** One configured provider for the management list. Never carries the api key
 *  (it lives only in the keyring). */
export interface AiProviderView {
  id: string;
  name: string;
  kind: ProviderKind;
  model: string | null;
  active: boolean;
}

// --- Send gate ------------------------------------------------------------

/**
 * Whether Send is enabled: the input must be non-blank, and Suggest mode also
 * requires a connection (its schema is what the backend attaches). Explain
 * runs with no connection — it sends only the SQL text.
 */
export function canSend(
  mode: AiMode,
  input: string,
  hasConnection: boolean,
): boolean {
  if (input.trim().length === 0) return false;
  return mode === 'explain' || hasConnection;
}

/** The "include column details" toggle only makes sense in Suggest mode. */
export function showIncludeDetails(mode: AiMode): boolean {
  return mode === 'suggest';
}

// --- Streaming fold -------------------------------------------------------

/** Running totals for one in-flight stream, folded from `AiChunk`s. */
export interface StreamState {
  text: string;
  tokensIn: number;
  tokensOut: number;
}

export function emptyStream(): StreamState {
  return { text: '', tokensIn: 0, tokensOut: 0 };
}

/**
 * Fold one chunk in: append its delta text and *replace* the cumulative token
 * counts (each chunk already carries the running total, so summing would
 * double-count). Pure — returns a new state, mirrors the Rust `StreamAcc`.
 */
export function accumulate(state: StreamState, chunk: AiChunk): StreamState {
  return {
    text: state.text + chunk.text_delta,
    tokensIn: chunk.tokens_in,
    tokensOut: chunk.tokens_out,
  };
}

/** Whether the token meter has anything to show yet (avoids a "0 in / 0 out"
 *  flash before the first usage event). */
export function hasTokens(tokensIn: number, tokensOut: number): boolean {
  return tokensIn > 0 || tokensOut > 0;
}

// --- Provider form model --------------------------------------------------

export type ProviderMode = 'add' | 'edit';

/** Display order in the kind picker. Anthropic first: it is the default the
 *  env-var fallback also builds. */
export const PROVIDER_KINDS: readonly ProviderKind[] = [
  'anthropic',
  'openai',
] as const;

export interface ProviderForm {
  id: string;
  name: string;
  kind: ProviderKind;
  model: string;
  /** Blank on edit means "keep the stored key" (parity with the connection
   *  form, ADR-0016); on add it is required. */
  apiKey: string;
}

export type ProviderField = 'id' | 'name' | 'apiKey';

export function emptyProviderForm(): ProviderForm {
  return { id: '', name: '', kind: 'anthropic', model: '', apiKey: '' };
}

/** Seed the edit form from an existing provider. The api key stays blank (it
 *  is never read back out of the keyring); the kind is fixed. */
export function providerFormForEdit(view: AiProviderView): ProviderForm {
  return {
    id: view.id,
    name: view.name,
    kind: view.kind,
    model: view.model ?? '',
    apiKey: '',
  };
}

const blank = (v: string): boolean => v.trim().length === 0;

/**
 * Required-but-blank fields (empty ⇒ valid). Add requires id, name, and a key;
 * edit requires only name (id is immutable, a blank key keeps the stored one,
 * the model is always optional).
 */
export function validateProvider(
  form: ProviderForm,
  mode: ProviderMode,
): ProviderField[] {
  const required: ProviderField[] =
    mode === 'add' ? ['id', 'name', 'apiKey'] : ['name'];
  return required.filter((f) => blank(form[f]));
}

/** Trim a model to `undefined` when blank so the backend's `none_if_blank`
 *  falls back to the provider's default model. */
export function normalizeModel(raw: string): string | undefined {
  const trimmed = raw.trim();
  return trimmed.length === 0 ? undefined : trimmed;
}

/**
 * The `kind` object `add_ai_provider` expects (a tagged `AiKindInput`): the
 * discriminator plus an optional model and the required key.
 */
export function buildAddKindInput(form: ProviderForm): Record<string, unknown> {
  return {
    kind: form.kind,
    model: normalizeModel(form.model) ?? null,
    api_key: form.apiKey,
  };
}
