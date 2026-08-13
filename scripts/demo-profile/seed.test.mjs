// The corpus that ends up in the download page's screenshots.
//
// Two things are being pinned here, and only one of them is about SQL.
//
// The first is that the pictures show the app doing something. A screenshot of
// three tables with four rows each proves the window opens, not that the app is
// worth installing. So: enough tables to fill the sidebar, one table big enough
// that a row limit is visibly holding something back, and every column type the
// grid can render — including a null, which is the one cell people look for.
//
// The second is that nothing in the picture belongs to anybody. These images go
// on a public page, and the reason they did not exist until now is that a
// capture of the app as installed shows a real database host in the connection
// list. A fictional corpus is the fix, but "fictional" has to be checked rather
// than intended: no personal names, no company names, no real places, nothing
// shaped like an email address or a host. Hence the tests at the bottom, which
// are the ones worth keeping if the rest ever gets deleted.

import assert from "node:assert/strict";
import { test } from "node:test";
import { TABLES, buildRows } from "./seed.mjs";

test("the sidebar has enough tables to look like a database", () => {
  assert.ok(
    TABLES.length >= 3,
    `only ${TABLES.length} table(s); the sidebar would read as a toy`,
  );
});

test("one table is big enough that a row limit visibly does something", () => {
  const counts = TABLES.map((t) => buildRows(t.name).length);
  assert.ok(
    counts.some((n) => n > 100),
    `largest table has ${Math.max(...counts)} rows; under a 100-row limit nothing is held back and the limit cannot be seen working`,
  );
});

test("every column type the grid renders appears somewhere", () => {
  const seen = new Set();
  for (const { name } of TABLES) {
    for (const row of buildRows(name)) {
      for (const value of Object.values(row)) {
        seen.add(value === null ? "null" : typeof value);
      }
    }
  }
  for (const want of ["string", "number", "null"]) {
    assert.ok(seen.has(want), `no ${want} anywhere in the corpus`);
  }
  // Integers and reals render differently and are worth having both of.
  const numbers = TABLES.flatMap(({ name }) =>
    buildRows(name).flatMap((row) =>
      Object.values(row).filter((v) => typeof v === "number"),
    ),
  );
  assert.ok(
    numbers.some((n) => Number.isInteger(n)),
    "no integer values",
  );
  assert.ok(
    numbers.some((n) => !Number.isInteger(n)),
    "no fractional values, so the grid never shows a real",
  );
});

test("ids are unique within each table", () => {
  for (const { name } of TABLES) {
    const ids = buildRows(name).map((row) => row.id);
    assert.equal(
      new Set(ids).size,
      ids.length,
      `${name} has duplicate ids, so a screenshot of it would show a table no primary key would allow`,
    );
  }
});

test("the corpus is deterministic", () => {
  // Screenshots get retaken every release. If the numbers move each run, the
  // diff between two releases' images is noise and nobody can tell whether the
  // app changed or the data did.
  for (const { name } of TABLES) {
    assert.deepEqual(buildRows(name), buildRows(name), `${name} is not stable`);
  }
});

test("nothing in the corpus belongs to a real person, company or place", () => {
  // Not a substitute for reading it. It catches the shapes that leak by
  // accident -- an address pasted in as sample data, a hostname left in a note
  // field -- because those are what survive review.
  const text = TABLES.flatMap(({ name }) =>
    buildRows(name).flatMap((row) =>
      Object.values(row).filter((v) => typeof v === "string"),
    ),
  );
  for (const value of text) {
    assert.doesNotMatch(value, /@/, `"${value}" is shaped like an email address`);
    assert.doesNotMatch(
      value,
      /\b\d{1,3}(\.\d{1,3}){3}\b/,
      `"${value}" is shaped like an IP address`,
    );
    assert.doesNotMatch(
      value,
      /\b[a-z0-9-]+\.(com|net|org|jp|co\.jp|io|dev)\b/i,
      `"${value}" is shaped like a domain name`,
    );
  }
});
