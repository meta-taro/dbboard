# Firestore emulator

A local Cloud Firestore to point dbboard at, so that
[`docs/test-specs/001-firestore-connection.tsv`](../../docs/test-specs/001-firestore-connection.tsv)
can be worked through without a Google Cloud project, a billing account or a
service-account key.

Everything lives in memory. Stopping the container throws the data away, which
is the point: there is nothing here to leak and nothing to clean up.

## Start it

```sh
docker compose -f docker/firestore-emulator/compose.yaml up -d
node docker/firestore-emulator/seed.mjs
```

The first command pulls Google's Cloud SDK image on a cold machine, which is
about a gigabyte and takes a few minutes. The second waits for the emulator to
answer before it writes anything, so running both back to back is fine.

`seed.mjs` needs no packages — it is Node's built-in `fetch` and nothing else.

## Point dbboard at it

Add a connection with these values. There is no key file and no password.

| Field | Value |
|---|---|
| Kind | Firestore |
| Project ID | `demo-dbboard` |
| Database | leave empty (`(default)`) |
| Base URL | `http://127.0.0.1:8385/v1` |
| Credentials | leave empty — an empty credential *is* the emulator setting (ADR-0094) |

Three collections appear in the sidebar:

| Collection | Documents | What it is for |
|---|---|---|
| `dbboard_demo_catalogue` | 12 | Every scalar type the adapter maps, including nulls |
| `dbboard_demo_events` | 130 | More than 100, so a `limit` that is ignored is visible |
| `dbboard_demo_nested` | 2 | Maps and arrays, including a map inside an array, and an empty one of each |

## What this cannot answer

Rows 2 through 9 of sheet 001 are answerable against the emulator. Three rows
are not, and they should stay `未実施` until a real project is available:

- **Row 1** — connecting with a service-account key. The emulator has no
  authentication of any kind, so there is no key to supply.
- **Row 10** — a readable error for a wrong project ID. The emulator serves
  whatever project ID it is asked for, so a wrong one returns an empty
  database rather than an error. Passing this row here would prove nothing.
- **Row 11** — a readable error for invalid credentials, for the same reason
  as row 1.

Answering a row against the emulator when the row is about credentials would
put an `OK` next to a claim nobody tested. Leave those three alone.

## Stop it

```sh
docker compose -f docker/firestore-emulator/compose.yaml down
```

## Also used by the adapter tests

The live adapter tests in `crates/dbboard-firestore/tests/live_firestore.rs`
take the same emulator. They seed their own collections, so they do not
conflict with the corpus above:

```sh
DBBOARD_TEST_FIRESTORE_URL=http://127.0.0.1:8385/v1 \
  DBBOARD_TEST_FIRESTORE_PROJECT=demo-dbboard \
  cargo test -p dbboard-firestore --test live_firestore -- --ignored
```

Those tests skip rather than fail when the URL is unset, so a machine with no
emulator running is not a broken machine.

## Checking the corpus itself

```sh
node --test docker/firestore-emulator/seed.test.mjs
```

Runs without Docker. It asserts the shape the sheet depends on — that one
collection exceeds 100 documents, that the nested collection really contains
both a map and an array, that every value uses Firestore's typed encoding.
