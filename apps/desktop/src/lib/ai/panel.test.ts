import { describe, expect, it } from 'vitest';
import {
  canSend,
  showIncludeDetails,
  emptyStream,
  accumulate,
  hasTokens,
  emptyProviderForm,
  providerFormForEdit,
  validateProvider,
  normalizeModel,
  buildAddKindInput,
  PROVIDER_KINDS,
  type AiChunk,
  type AiProviderView,
  type ProviderField,
  type ProviderForm,
} from './panel';

describe('canSend', () => {
  it('is false while the input is blank, whitespace included', () => {
    expect(canSend('explain', '', false)).toBe(false);
    expect(canSend('explain', '   \n', false)).toBe(false);
    expect(canSend('suggest', '', true)).toBe(false);
  });

  it('lets Explain run with no connection — it sends only the SQL text', () => {
    expect(canSend('explain', 'SELECT 1', false)).toBe(true);
  });

  it('requires a connection for Suggest — its schema is attached', () => {
    expect(canSend('suggest', 'top customers', false)).toBe(false);
    expect(canSend('suggest', 'top customers', true)).toBe(true);
  });
});

describe('showIncludeDetails', () => {
  it('offers the column-details toggle only in Suggest mode', () => {
    expect(showIncludeDetails('suggest')).toBe(true);
    expect(showIncludeDetails('explain')).toBe(false);
  });
});

describe('accumulate', () => {
  const chunk = (over: Partial<AiChunk> = {}): AiChunk => ({
    text_delta: '',
    tokens_in: 0,
    tokens_out: 0,
    ...over,
  });

  it('appends delta text across chunks', () => {
    let s = emptyStream();
    s = accumulate(s, chunk({ text_delta: 'SELECT ' }));
    s = accumulate(s, chunk({ text_delta: '1' }));
    expect(s.text).toBe('SELECT 1');
  });

  it('replaces the cumulative token counts, never sums them', () => {
    let s = emptyStream();
    s = accumulate(s, chunk({ tokens_in: 10, tokens_out: 3 }));
    s = accumulate(s, chunk({ tokens_in: 10, tokens_out: 7 }));
    expect(s.tokensIn).toBe(10);
    expect(s.tokensOut).toBe(7);
  });

  it('does not mutate the input state', () => {
    const s0 = emptyStream();
    const s1 = accumulate(s0, chunk({ text_delta: 'x', tokens_out: 1 }));
    expect(s0).toEqual({ text: '', tokensIn: 0, tokensOut: 0 });
    expect(s1).not.toBe(s0);
  });
});

describe('hasTokens', () => {
  it('is false before any usage arrives', () => {
    expect(hasTokens(0, 0)).toBe(false);
  });

  it('is true once either count is positive', () => {
    expect(hasTokens(5, 0)).toBe(true);
    expect(hasTokens(0, 2)).toBe(true);
  });
});

describe('validateProvider', () => {
  const form = (over: Partial<ProviderForm> = {}): ProviderForm => ({
    ...emptyProviderForm(),
    ...over,
  });

  it('requires id, name, and a key to add', () => {
    expect(validateProvider(form(), 'add').sort()).toEqual<ProviderField[]>([
      'apiKey',
      'id',
      'name',
    ]);
  });

  it('passes a complete add form', () => {
    expect(
      validateProvider(
        form({ id: 'a', name: 'A', apiKey: 'sk-1' }),
        'add',
      ),
    ).toEqual([]);
  });

  it('requires only a name on edit — a blank key keeps the stored one', () => {
    expect(validateProvider(form({ name: '' }), 'edit')).toEqual<
      ProviderField[]
    >(['name']);
    expect(
      validateProvider(form({ name: 'A', apiKey: '' }), 'edit'),
    ).toEqual([]);
  });
});

describe('normalizeModel', () => {
  it('collapses a blank model to undefined (backend picks the default)', () => {
    expect(normalizeModel('')).toBeUndefined();
    expect(normalizeModel('   ')).toBeUndefined();
  });

  it('trims and keeps a real model name', () => {
    expect(normalizeModel('  claude-sonnet-5 ')).toBe('claude-sonnet-5');
  });
});

describe('providerFormForEdit', () => {
  const view = (over: Partial<AiProviderView> = {}): AiProviderView => ({
    id: 'a',
    name: 'A',
    kind: 'anthropic',
    model: null,
    active: false,
    ...over,
  });

  it('seeds id/name/kind/model but leaves the key blank', () => {
    const f = providerFormForEdit(view({ model: 'gpt-4o', kind: 'openai' }));
    expect(f).toEqual<ProviderForm>({
      id: 'a',
      name: 'A',
      kind: 'openai',
      model: 'gpt-4o',
      apiKey: '',
    });
  });

  it('maps a null model to an empty string', () => {
    expect(providerFormForEdit(view()).model).toBe('');
  });
});

describe('buildAddKindInput', () => {
  it('carries the discriminator, model, and key the backend parses', () => {
    const kind = buildAddKindInput({
      id: 'a',
      name: 'A',
      kind: 'openai',
      model: 'gpt-4o',
      apiKey: 'sk-1',
    });
    expect(kind).toEqual({ kind: 'openai', model: 'gpt-4o', api_key: 'sk-1' });
  });

  it('sends a null model when blank so the default is used', () => {
    const kind = buildAddKindInput({
      id: 'a',
      name: 'A',
      kind: 'anthropic',
      model: '  ',
      apiKey: 'sk-1',
    });
    expect(kind).toEqual({ kind: 'anthropic', model: null, api_key: 'sk-1' });
  });
});

describe('PROVIDER_KINDS', () => {
  it('lists anthropic first, then openai', () => {
    expect(PROVIDER_KINDS).toEqual(['anthropic', 'openai']);
  });
});
