// Checks on the seed corpus, run without Docker and without a running
// emulator:
//
//   node --test docker/firestore-emulator/seed.test.mjs
//
// The seeding itself is a handful of HTTP POSTs and is not worth a test. What
// is worth pinning is the *shape* of the corpus, because the corpus exists to
// make specific rows of `docs/test-specs/001-firestore-connection.tsv`
// answerable. If somebody trims it, the sheet quietly stops proving what it
// claims to prove: a "Select top 100" row against 12 documents cannot tell a
// working limit from an ignored one.
import { test } from "node:test";
import assert from "node:assert/strict";
import { COLLECTIONS } from "./seed.mjs";

const collection = (name) => {
  const found = COLLECTIONS.find((c) => c.name === name);
  assert.ok(found, `the corpus no longer has a collection named ${name}`);
  return found;
};

test("more than one collection, so the sidebar list is a list", () => {
  // Sheet 001 row 5 reads "collection names line up in the sidebar". One
  // collection would render as one line and prove nothing about listing.
  assert.ok(
    COLLECTIONS.length >= 3,
    `only ${COLLECTIONS.length} collections — row 5 needs a list, not an entry`,
  );
});

test("one collection holds more than 100 documents", () => {
  // Sheet 001 row 6: "Select top 100 ... returns 100 rows or fewer". Against a
  // collection of 12 that passes whether or not the limit is applied at all.
  const many = COLLECTIONS.filter((c) => c.documents.length > 100);
  assert.ok(
    many.length >= 1,
    "no collection exceeds 100 documents, so row 6 cannot distinguish a " +
      "working limit from an ignored one",
  );
});

test("the nested collection has both a map and an array", () => {
  // Sheet 001 row 8: nested map / array must not render as blank or
  // `[object Object]`. Both containers are named because they fail
  // differently — an array can render as a length, a map as its type name.
  const { documents } = collection("dbboard_demo_nested");
  const kinds = new Set(
    documents.flatMap((doc) => Object.values(doc.fields).map((v) => Object.keys(v)[0])),
  );
  assert.ok(kinds.has("mapValue"), "no mapValue anywhere in dbboard_demo_nested");
  assert.ok(kinds.has("arrayValue"), "no arrayValue anywhere in dbboard_demo_nested");
});

test("every document is in Firestore's typed-value encoding", () => {
  // The REST API takes `{"fields": {"name": {"stringValue": "a"}}}`, not
  // `{"name": "a"}`. A plain value is accepted by nothing and fails at seed
  // time with a 400 that does not say which document was wrong.
  const typed = new Set([
    "nullValue",
    "booleanValue",
    "integerValue",
    "doubleValue",
    "timestampValue",
    "stringValue",
    "bytesValue",
    "referenceValue",
    "geoPointValue",
    "arrayValue",
    "mapValue",
  ]);

  for (const { name, documents } of COLLECTIONS) {
    for (const { id, fields } of documents) {
      assert.ok(id, `a document in ${name} has no id`);
      for (const [field, value] of Object.entries(fields)) {
        const keys = Object.keys(value);
        assert.equal(
          keys.length,
          1,
          `${name}/${id}.${field} has ${keys.length} type keys, expected exactly 1`,
        );
        assert.ok(
          typed.has(keys[0]),
          `${name}/${id}.${field} uses ${keys[0]}, which is not a Firestore value type`,
        );
      }
    }
  }
});

test("document ids are unique within a collection", () => {
  // Firestore's create endpoint takes the id as a query parameter and
  // overwrites on a repeat, so a duplicate would silently shrink the corpus —
  // and the count row would disagree with this file for no visible reason.
  for (const { name, documents } of COLLECTIONS) {
    const ids = documents.map((d) => d.id);
    assert.equal(
      new Set(ids).size,
      ids.length,
      `${name} has duplicate document ids`,
    );
  }
});

test("the collection names cannot be mistaken for a real database", () => {
  // This corpus is seeded into whatever base URL it is pointed at. The prefix
  // is the last line of defence if somebody aims it at a project that holds
  // real data: a collection called `orders` would merge into theirs, one
  // called `dbboard_demo_orders` would not. No exceptions — an allow-list with
  // one bare name in it is how the prefix stops being a rule.
  for (const { name } of COLLECTIONS) {
    assert.match(name, /^dbboard_demo_/, `${name} does not carry the demo prefix`);
  }
});
