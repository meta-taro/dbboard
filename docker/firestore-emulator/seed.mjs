// Fills a freshly started Firestore emulator with something to look at.
//
//   node docker/firestore-emulator/seed.mjs
//
// An emulator starts empty, and dbboard cannot fill it: neither the Firestore
// nor the MongoDB adapter implements a write path, which is the property the
// read-only guarantee rests on. So seeding goes over plain REST, exactly as
// `crates/dbboard-firestore/tests/live_firestore.rs` does.
//
// The corpus is shaped by `docs/test-specs/001-firestore-connection.tsv`.
// `seed.test.mjs` pins that shape; read the comments there before trimming
// anything here.
import { pathToFileURL } from "node:url";

const BASE_URL =
  process.env.DBBOARD_TEST_FIRESTORE_URL?.trim() || "http://127.0.0.1:8385/v1";
const PROJECT =
  process.env.DBBOARD_TEST_FIRESTORE_PROJECT?.trim() || "demo-dbboard";

const string = (v) => ({ stringValue: v });
const int = (v) => ({ integerValue: String(v) });
const double = (v) => ({ doubleValue: v });
const bool = (v) => ({ booleanValue: v });
const timestamp = (v) => ({ timestampValue: v });
const nothing = () => ({ nullValue: null });
const array = (values) => ({ arrayValue: { values } });
const map = (fields) => ({ mapValue: { fields } });

// Twelve rows of ordinary scalars. Small enough that a human can count the
// sidebar against "Count rows" (sheet 001 row 7) without scrolling, and wide
// enough that the grid has to render every value type the adapter maps:
// string, integer, double, boolean, timestamp and null.
const catalogue = Array.from({ length: 12 }, (_, i) => ({
  id: `item-${String(i + 1).padStart(2, "0")}`,
  fields: {
    name: string(`Sample item ${i + 1}`),
    sku: string(`SKU-${1000 + i}`),
    quantity: int(i * 3),
    unit_price: double(Number((9.5 + i * 1.25).toFixed(2))),
    in_stock: bool(i % 3 !== 0),
    updated_at: timestamp(`2026-08-${String((i % 28) + 1).padStart(2, "0")}T09:00:00Z`),
    // Deliberately null on some rows: an empty cell and a null cell look the
    // same in a grid that does not distinguish them, and that is worth seeing.
    discontinued_at: i % 4 === 0 ? timestamp("2026-07-01T00:00:00Z") : nothing(),
  },
}));

// 130 documents — more than the 100 that "Select top 100" asks for, so the row
// distinguishes a limit that works from one that is ignored. Ordered by `n` so
// a human can tell at a glance where the result stopped.
const events = Array.from({ length: 130 }, (_, i) => ({
  id: `event-${String(i + 1).padStart(3, "0")}`,
  fields: {
    n: int(i + 1),
    kind: string(["opened", "closed", "retried"][i % 3]),
    at: timestamp(`2026-08-01T${String(i % 24).padStart(2, "0")}:30:00Z`),
  },
}));

// Nested containers, the case sheet 001 row 8 is about. Both a map and an
// array, and a map inside an array, because a grid that special-cases only the
// top level will render the inner one as `[object Object]`.
const nested = [
  {
    id: "order-1001",
    fields: {
      reference: string("ORD-1001"),
      customer: map({
        name: string("Sample Customer"),
        tier: string("standard"),
        address: map({ city: string("Springfield"), postcode: string("00000") }),
      }),
      lines: array([
        map({ sku: string("SKU-1000"), quantity: int(2) }),
        map({ sku: string("SKU-1003"), quantity: int(1) }),
      ]),
      tags: array([string("paid"), string("shipped")]),
      total: double(48.75),
    },
  },
  {
    id: "order-1002",
    fields: {
      reference: string("ORD-1002"),
      customer: map({ name: string("Another Customer"), tier: string("trial") }),
      // An empty array and an empty map: the two containers most likely to be
      // rendered as blank when the non-empty ones render fine.
      lines: array([]),
      tags: array([string("draft")]),
      metadata: map({}),
      total: double(0),
    },
  },
];

export const COLLECTIONS = [
  { name: "dbboard_demo_catalogue", documents: catalogue },
  { name: "dbboard_demo_events", documents: events },
  { name: "dbboard_demo_nested", documents: nested },
];

const documentsRoot = () =>
  `${BASE_URL}/projects/${PROJECT}/databases/(default)/documents`;

/// The emulator listens a moment after the container reports itself up, and
/// `compose up -d` returns before then. Poll rather than sleep a guessed
/// number of seconds.
async function waitForEmulator(attempts = 60) {
  for (let i = 1; i <= attempts; i += 1) {
    try {
      const response = await fetch(documentsRoot());
      // Any answer at all means it is listening. A 404 on an empty database is
      // still an answer.
      if (response.status < 500) return;
    } catch {
      // Connection refused: not up yet.
    }
    if (i === attempts) {
      throw new Error(
        `no Firestore emulator answered at ${BASE_URL} after ${attempts} tries.\n` +
          "Start it with: docker compose -f docker/firestore-emulator/compose.yaml up -d",
      );
    }
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
}

async function seedCollection({ name, documents }) {
  const root = documentsRoot();

  // The emulator has no "drop collection", so a re-run deletes the ids it is
  // about to write. Anything a human added by hand under a different id
  // survives, which is the behaviour you want while working through a sheet.
  await Promise.all(
    documents.map((doc) =>
      fetch(`${root}/${name}/${doc.id}`, { method: "DELETE" }).catch(() => {}),
    ),
  );

  for (const doc of documents) {
    const response = await fetch(
      `${root}/${name}?documentId=${encodeURIComponent(doc.id)}`,
      {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ fields: doc.fields }),
      },
    );
    if (!response.ok) {
      throw new Error(
        `seeding ${name}/${doc.id} failed: ${response.status} ${await response.text()}`,
      );
    }
  }

  console.log(`  ${name}: ${documents.length} documents`);
}

async function main() {
  console.log(`Seeding ${PROJECT} at ${BASE_URL}`);
  await waitForEmulator();
  for (const collection of COLLECTIONS) {
    await seedCollection(collection);
  }
  console.log("Done.");
}

// Only seed when run directly, so `seed.test.mjs` can import the corpus
// without needing an emulator. `pathToFileURL` rather than string-building the
// URL: on Windows the path is `C:\...`, and a hand-rolled `file://` + path
// gets both the drive letter and the separators wrong.
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
