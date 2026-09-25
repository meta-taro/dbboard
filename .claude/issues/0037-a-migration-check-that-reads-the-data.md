# 0037: the two halves of a migration check that an agent cannot do by itself

- **Status**: open (planning — nothing is being built yet)
- **Phase**: 7 (Migration), with an argument for a slice much earlier
- **Opened**: 2026-09-25
- **From**: [`.claude/plans/2026-08-17-database-workspace.md`](../plans/2026-08-17-database-workspace.md)
  §4 Compatibility Advisor, §9 Migration Pre-flight, §11 Migration Validation,
  §14 Migration Readiness

## The failure this is about

From the maintainer, who has done this work:

> **mysql とかでも sql が同じでも返る値が変わる事があったはずです**

**Same engine. Same schema, byte for byte. Same SQL. Different answer.**

MySQL 5.7 → 8.0, the changes that alter what comes back rather than how fast:

| Change | What the application sees |
|---|---|
| Default collation → `utf8mb4_0900_ai_ci` | **`ORDER BY` returns a different order.** The new one is accent-insensitive, so `=` starts matching rows it used to reject |
| Implicit `GROUP BY` sort removed | **Order changes with no error at all** |
| New reserved words (`RANK`, `WINDOW`, `LEAD`, …) | A column named one of them is now a syntax error |
| `explicit_defaults_for_timestamp` on | `TIMESTAMP` NULL handling and auto-update change |
| `LIMIT` without `ORDER BY` | A different plan returns **different rows**, and both are correct |

5.6 → 5.7 has its own set: `ONLY_FULL_GROUP_BY` by default, and a stricter
`sql_mode` turning silent truncation into refusal.

**And it does not even need a database version to move.** The glibc 2.28
collation change reorders text and invalidates indexes on an OS upgrade, with
PostgreSQL untouched.

**A schema comparison sees none of this.** Both sides agree on every
structural measure the compare tab reports. It would say so, correctly, and
the application would still break.

*(These specifics are from memory and must be checked against release notes
before any of them is encoded in the product.)*

## Why a knowledge base is the wrong shape

§4's Compatibility Advisor points at a table of "version X changes Y". That
table is always behind, needs an entry per pair, and **the glibc case shows
the change can arrive from outside the version number entirely** — so the
framing cannot cover its own worst example.

**Asking the same question twice needs no table.** Send the query to both,
compare the answers. Nothing has to be known in advance, nothing goes stale,
and a MySQL released tomorrow works with no code change.

## What an agent can already do today

**The comparison loop needs no new code.** `list_connections` and
`run_read_query` are on the MCP surface now:

```
list_connections            → pick source and target
run_read_query(left,  sql)
run_read_query(right, sql)
compare                     → the agent's own judgement
```

By [ADR-0087](../../docs/decisions.md)'s test — a verb is additive only while
it opens nothing an operator could not already open by hand — **a
`compare_query` verb would open nothing.** It is not worth adding.

**With an agent in the loop, three of the four limits in the way disappear:**

| Limit | With an agent |
|---|---|
| Telling "differs because of the migration" from "differs every run" (`NOW()`, `RAND()`, generated ids, unordered results) | **Reading the SQL answers it.** No rule engine can; judgement can |
| Queries do not port across engines — an aggregation pipeline cannot be sent to Firestore | **The agent writes the target's equivalent.** This was the human work that made cross-engine comparison expensive |
| Coverage is limited to the queries on hand | **The agent can read the application for its queries** |

That third row matters more than it looks. **An earlier draft of this issue
claimed saved queries and history were already the input. History is not
written at all** — `history.jsonl` has no writer anywhere in the workspace
([issue 0033](0033-history-jsonl-has-no-writer.md)) — **and saved queries are
not on the MCP surface.** The material this was supposed to stand on does not
exist; reading the application does not depend on it.

The one limit that stays: **both sides have to be standing, with data in
them.** This is a rehearsal tool, not something to consult before deciding.

## So dbboard builds the two things an agent cannot

### 1. The record, and a person's signature on it

**An agent comparing five hundred queries produces findings that live in a
conversation and then vanish.** Nothing survives to justify the decision.

git-qa already solved this shape, in this organisation, and enforces it at the
type level: **`AUTO_PASS` means nobody looked; `VERIFIED` means a person did.**
The same split is exactly right here. **The agent narrows five hundred
differences to the few that matter; a person signs those.** What remains is a
document that answers "is this migration safe" with reasons attached — §14's
BLOCKER / WARNING / SAFE, never a single score, because 83% reads as *probably
fine* and one row over a limit is what stops the move.

### 2. A scan for whether the data fits the target at all

Mongo → Firestore needs a question asked before any query: **does this data
fit?**

- **Document size** — MongoDB allows 16 MiB, Firestore about 1 MiB
- **Nesting depth**, and **arrays inside arrays**, which Firestore refuses
- **Field names** the target reserves or rejects
- **Types with no counterpart**, `Decimal128` being the plain one
- **Identity** — `ObjectId` is not a Firestore document id

**An agent cannot do this through `run_read_query`.** It is a pass over every
document, and answering it that way moves the whole collection through the
agent. Counting server-side and returning the offenders is genuinely new by
ADR-0087's test — it opens something no operator could open by hand.

**Rules attach to the target's limits alone.** What Firestore refuses is true
whatever the source was, so this is one rule set per target, not one per pair:
eleven adapters give eleven, not fifty-five.

## What must not happen

**Never report an unexamined thing as a pass.** The failure ADR-0148 leaned
against — calling two things the same — appears here as calling an unchecked
thing safe. A target with no rules written must say *nothing is known*.

**Never claim the application is covered.** A query returning the same rows in
a different order passes every check here and still breaks a page that
assumed the order.

**Read-only must be enforced, not assumed.** Two production connections being
sent the same statement is precisely when a stray write matters most.
`run_read_query` is the door; the record must show which door was used.

## Open questions

- **How much earlier than band 7?** Both halves are smaller than the band's
  position implies, because the comparison itself is already possible.
  Displacing a band is the maintainer's call, and slots move forward rather
  than compress.
- **What counts as the same answer?** Float rounding, timestamp precision and
  declared type strings differ for reasons nobody cares about. ADR-0148 chose
  to over-report rather than miss. **Whether that holds across hundreds of
  queries is not obvious** — at that volume over-reporting buries the finding
  it was protecting. This is ADR-0148's question again, one layer up, and it
  deserves its own ADR.
- **Where does the signing happen?** git-qa is a separate product. Whether
  dbboard grows its own signed-run format, or hands the comparison to git-qa
  as a sheet, decides how much is built here.
- **Band 7 or band 8?** The roadmap puts the *AI* compatibility advisor in
  band 8. Neither half here needs a model — one is a record, the other is a
  counting pass.

## Acceptance (planning only)

- [ ] Decide whether a slice moves earlier than band 7, and what it displaces
- [ ] Decide what "the same answer" means; record it as an ADR
- [ ] Decide whether the record is dbboard's own or a git-qa sheet
- [ ] Write the first target's rule set as rules, before any code
- [ ] Verify the version-change specifics above against release notes

## Notes

The plan file is stored verbatim and not edited; this issue is derived from
it, per `CLAUDE.md`.

**Two earlier drafts of this file opened with process** — "it has sat
unchecked in Phase 5", then "the reservation is too far away". Both are
reasons to a maintainer and to nobody else. `site/index.html`'s compare
section has the same shape: it explains the mechanism thoroughly and never
says when a person would need it. The landing page rewrite fixed the headline
and left the sections alone.
