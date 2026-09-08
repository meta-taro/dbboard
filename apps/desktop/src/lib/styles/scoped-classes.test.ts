import { describe, expect, it } from 'vitest';

// Svelte scopes a component's <style> to that component, and nothing warns
// when markup names a class the component cannot reach: `svelte-check` type
// checks, and the compiler warns about selectors with no element — never about
// an element with no selector.
//
// It shipped once, in v0.16.0. The saved-queries button carried
// `class="chip"`, whose rules live in QueryPanel, and reached Windows as a
// bare browser button sitting next to a styled one.
//
// The check is deliberately narrow: it flags a class only when the rules
// EXIST somewhere else in the app and this component cannot reach them —
// which is exactly the mistake above. A class with no rules anywhere is a
// marker or a leftover hook: harmless, and not this test's business.
//
// Sources are read through `import.meta.glob` rather than `node:fs` so the
// test needs no Node types (`@types/node` is not a dependency here — the CI
// runner found that out before this comment existed) and no assumption about
// the working directory.

// The options must be written out at each call: Vite reads them at build
// time, so a shared constant is not something it can follow.
const componentSources = import.meta.glob('../components/*.svelte', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>;
const routeSources = import.meta.glob('../../routes/*.svelte', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>;
const sheetSources = import.meta.glob('./*.css', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>;

const basename = (path: string): string => path.slice(path.lastIndexOf('/') + 1);

const components = Object.entries(componentSources).map(([path, source]) => ({
  file: basename(path),
  source,
}));
const all = [
  ...components,
  ...Object.entries(routeSources).map(([path, source]) => ({
    file: basename(path),
    source,
  })),
];

const classNames = (css: string): string[] =>
  [...css.matchAll(/\.([A-Za-z][\w-]*)/g)].map((m) => m[1]);

const styleBlock = (source: string): string => {
  const at = source.indexOf('<style>');
  return at === -1 ? '' : source.slice(at);
};

const markupOf = (source: string): string => {
  const at = source.indexOf('<style>');
  return at === -1 ? source : source.slice(0, at);
};

function classesUsedIn(markup: string): Set<string> {
  const used = new Set<string>();
  for (const m of markup.matchAll(/class="([^"{]+)"/g)) {
    for (const name of m[1].split(/\s+/)) if (name) used.add(name);
  }
  for (const m of markup.matchAll(/class:([A-Za-z][\w-]*)/g)) used.add(m[1]);
  return used;
}


/** Classes any component can use: global sheets, plus `:global(...)` rules. */
const globallyReachable = new Set<string>([
  ...Object.values(sheetSources).flatMap((css) => classNames(css)),
  ...all.flatMap(({ source }) =>
    [...styleBlock(source).matchAll(/:global\(([^)]*)\)/g)].flatMap((m) =>
      classNames(m[1]),
    ),
  ),
]);

/**
 * Names that collide by coincidence, one line each saying why — the shape
 * `deny.toml` uses for an ignored advisory. A blanket escape hatch would hide
 * the next real one, which is the whole point of the check.
 */
const COINCIDENCES: Record<string, string[]> = {
  // The sidebar's `.conn` marks which rows are connections (`.nav-row` does
  // the styling); the dialogs' `.conn` labels a connection name. Same word,
  // unrelated rules.
  'Sidebar.svelte': ['conn'],
};

describe('a component never names a class whose rules it cannot reach', () => {
  it('finds the components', () => {
    // Without this the suite could pass by walking an empty directory.
    expect(components.length).toBeGreaterThan(10);
  });

  for (const { file, source } of all) {
    it(file, () => {
      const own = new Set(classNames(styleBlock(source)));
      const elsewhere = new Set(
        all
          .filter((c) => c.file !== file)
          .flatMap(({ source: other }) => classNames(styleBlock(other))),
      );

      const allowed = new Set(COINCIDENCES[file] ?? []);
      const unreachable = [...classesUsedIn(markupOf(source))].filter(
        (c) =>
          !own.has(c) &&
          !globallyReachable.has(c) &&
          elsewhere.has(c) &&
          !allowed.has(c),
      );

      expect(
        unreachable,
        `${file} uses ${unreachable.join(', ')} — styled in another component, ` +
          `so these render unstyled here. Define them locally, or move the rules ` +
          `into a sheet under $lib/styles.`,
      ).toEqual([]);
    });
  }
});
