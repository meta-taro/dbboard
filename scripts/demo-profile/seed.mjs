// Builds the SQLite file the download page's screenshots are taken against.
//
// Run it, point a throwaway dbboard profile at the result, and the app has
// something to show that belongs to nobody. See README.md in this directory.
//
// Deliberately a plain SQLite file rather than a container: the emulator
// fixtures need a Docker daemon, and a screenshot should not be blocked on one.
// libSQL reads the file as-is, so the Turso/libSQL adapter opens it unchanged.

import { DatabaseSync } from "node:sqlite";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));

export const TABLES = [
  {
    name: "stations",
    ddl: `CREATE TABLE stations (
      id              INTEGER PRIMARY KEY,
      code            TEXT    NOT NULL UNIQUE,
      label           TEXT    NOT NULL,
      latitude        REAL    NOT NULL,
      longitude       REAL    NOT NULL,
      elevation_m     INTEGER NOT NULL,
      active          INTEGER NOT NULL,
      commissioned_on TEXT    NOT NULL
    )`,
  },
  {
    name: "readings",
    ddl: `CREATE TABLE readings (
      id            INTEGER PRIMARY KEY,
      station_id    INTEGER NOT NULL REFERENCES stations(id),
      taken_at      TEXT    NOT NULL,
      temperature_c REAL    NOT NULL,
      humidity_pct  INTEGER NOT NULL,
      pressure_hpa  REAL    NOT NULL,
      note          TEXT
    )`,
  },
  {
    name: "alerts",
    ddl: `CREATE TABLE alerts (
      id         INTEGER PRIMARY KEY,
      station_id INTEGER NOT NULL REFERENCES stations(id),
      raised_at  TEXT    NOT NULL,
      severity   TEXT    NOT NULL,
      summary    TEXT    NOT NULL,
      cleared_at TEXT
    )`,
  },
];

// Terrain nouns, not place names. A screenshot that carries a real town in it
// is a screenshot somebody has to think about before publishing, and the whole
// point of this corpus is that nobody has to.
const LABELS = [
  "North Ridge",
  "Lower Basin",
  "Salt Flat",
  "Pine Hollow",
  "Granite Spur",
  "Cold Fork",
  "Wind Gap",
  "Dry Wash",
  "Iron Bluff",
  "Long Meadow",
  "Cedar Notch",
  "Slate Bench",
];

const NOTES = [
  null,
  null,
  null,
  "sensor swapped",
  "calibration drift",
  "manual entry",
  "housing iced over",
];

const SEVERITIES = ["info", "warning", "critical"];

// Fixed-seed PRNG rather than Math.random: screenshots are retaken every
// release, and a corpus that moves each run makes two releases' images differ
// for reasons that have nothing to do with the app.
function mulberry32(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const round = (value, places) => Number(value.toFixed(places));

const pad = (n, width) => String(n).padStart(width, "0");

/** UTC only: a local timezone would make the corpus depend on who ran it. */
function timestamp(dayOffset, hour) {
  const ms = Date.UTC(2026, 2, 1, hour) + dayOffset * 86_400_000;
  return new Date(ms).toISOString().replace(".000Z", "Z");
}

export function buildRows(table) {
  switch (table) {
    case "stations": {
      const rnd = mulberry32(0x5741_5f31);
      return LABELS.map((label, i) => ({
        id: i + 1,
        code: `ST-${pad(i + 1, 3)}`,
        label,
        latitude: round(34 + rnd() * 12, 4),
        longitude: round(132 + rnd() * 8, 4),
        elevation_m: 20 + Math.floor(rnd() * 1800),
        active: i % 7 === 3 ? 0 : 1,
        commissioned_on: `20${pad(14 + (i % 9), 2)}-${pad(1 + (i % 12), 2)}-${pad(1 + ((i * 3) % 27), 2)}`,
      }));
    }
    case "readings": {
      // Deliberately over a hundred: under dbboard's row limit the grid shows
      // everything, and a screenshot then cannot demonstrate that the limit is
      // doing anything at all.
      const rnd = mulberry32(0x5741_5f32);
      const rows = [];
      for (let i = 0; i < 480; i += 1) {
        const stationId = (i % LABELS.length) + 1;
        rows.push({
          id: i + 1,
          station_id: stationId,
          taken_at: timestamp(Math.floor(i / 24), i % 24),
          temperature_c: round(-4 + rnd() * 32, 1),
          humidity_pct: 28 + Math.floor(rnd() * 66),
          pressure_hpa: round(978 + rnd() * 46, 1),
          note: NOTES[Math.floor(rnd() * NOTES.length)],
        });
      }
      return rows;
    }
    case "alerts": {
      const rnd = mulberry32(0x5741_5f33);
      return Array.from({ length: 9 }, (_, i) => {
        const severity = SEVERITIES[i % SEVERITIES.length];
        return {
          id: i + 1,
          station_id: 1 + Math.floor(rnd() * LABELS.length),
          raised_at: timestamp(i * 2, 6 + (i % 12)),
          severity,
          summary: `${severity} threshold crossed on channel ${1 + (i % 4)}`,
          // Half of them still open, so the grid shows a null next to a value
          // in the same column rather than a column that is entirely empty.
          cleared_at: i % 2 === 0 ? timestamp(i * 2, 18 + (i % 5)) : null,
        };
      });
    }
    default:
      throw new Error(`no such table: ${table}`);
  }
}

function seed(path) {
  mkdirSync(dirname(path), { recursive: true });
  const db = new DatabaseSync(path);
  try {
    for (const { name, ddl } of TABLES) {
      db.exec(`DROP TABLE IF EXISTS ${name}`);
      db.exec(ddl);
      const rows = buildRows(name);
      const columns = Object.keys(rows[0]);
      const insert = db.prepare(
        `INSERT INTO ${name} (${columns.join(", ")}) VALUES (${columns.map(() => "?").join(", ")})`,
      );
      db.exec("BEGIN");
      for (const row of rows) {
        insert.run(...columns.map((column) => row[column]));
      }
      db.exec("COMMIT");
      process.stdout.write(`${name}: ${rows.length} rows\n`);
    }
  } finally {
    db.close();
  }
}

// `pathToFileURL` rather than string-building the URL: on Windows the path is
// `C:\...`, and a hand-rolled `file://` + path gets the drive letter and the
// separators both wrong.
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const path = process.argv[2] ?? join(HERE, "demo.db");
  seed(path);
  process.stdout.write(`\nwrote ${path}\n`);
}
