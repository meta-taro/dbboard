# 0021: What has to be true before v1.0

- **Status**: open — the four gates below are the whole list
- **Opened**: 2026-08-16
- **Owner**: split; see each gate
- **Related ADRs**: ADR-0011 (the HTTP contract is the public API for
  SemVer), ADR-0044 (signing placeholders), ADR-0055 (PII scanning),
  ADR-0084 (commit identity)

## What v1.0 means here

The public API for SemVer purposes is the HTTP contract in
`docs/api-contract.md`, not the feature set (ADR-0011). So **v1.0 is a
promise that this contract will not break without a major bump** — it is
not a claim that the feature list is finished.

That distinction decides everything below. Missing features do not block
v1.0, because adding an endpoint or a capability flag is additive and
breaks nothing. What blocks v1.0 is anything that would either force the
contract to change afterwards, or make the promise dishonest.

## The four gates

### 1. Issue #161 — the Run button does not respond to a click

Ctrl+Enter runs the query; the button does not. The primary action of
the primary screen. Nothing that ships as 1.0 should have this.

**Where it stands on 2026-08-16.** Two of the three candidate causes are
now closed by reading the code, and one observation was withdrawn as a
result:

- The button's `disabled={!connectionId || busy}` and the guard inside
  `execute` (`if (!connId || busy) return;`) are the **same expression**.
  So Ctrl+Enter succeeding proves the button was not disabled at that
  moment. The "is it greyed out?" question was unanswerable-by-observation
  in the first place, and was retracted.
- The CodeMirror autocomplete popup is `position: fixed` by default and
  does escape `.editor-host { overflow: hidden }`, so it *can* cover the
  Run button sitting directly below the editor — but it only exists while
  open, and `activateOnTyping` does not fire on paste or on a click that
  merely places the cursor. It does not match the reported steps.
- Its tooltip container is `view.dom` (no `parent` configured), so there
  is no always-present fixed overlay from the editor either.

Not reproducible by the maintainer on v0.8.0 — but that attempt used a
**MySQL** connection, and the report is against **Aurora DSQL (IAM)**, so
an adapter- or environment-specific cause is not excluded.

**Defined alternative, so this gate cannot stall v1.0 indefinitely.**
Like gate 4, this one has a second way out: **if the reporter's two
remaining observations do not arrive, ship 1.0 with the behaviour written
into `README.md` and the 1.0 release notes as a known issue, naming
Ctrl+Enter as the workaround and stating that its results are correct.**
The bug is in the desktop UI, not the HTTP contract, so it cannot force a
2.0 — which is the only thing v1.0 actually promises (ADR-0011). Shipping
a known UI defect that is documented and has a working alternative is
defensible; shipping it silently is not.

**Owner**: reporter, then whoever fixes it. Falls to the maintainer to
document if the reply does not come.

### 2. Freeze the contract, then mirror it to `dbboard-web`

`docs/roadmap.md` Phase 1.5 leaves one item unchecked: mirroring the
contract to the sibling repo. It has to happen *before* the freeze, not
after — once 1.0 is out, a change to the contract is a 2.0.

The drift found on 2026-08-16 is fixed and now has a regression test
(`crates/dbboard-connect/tests/api_contract_drift.rs`):

- the `id` list named 3 adapters; 9 ship
- `has_foreign_keys` (ADR-0054) was on the wire and absent from the document
- the `GET /capabilities` example disagreed with the `Capabilities` section
- three passages still described a state ("Phase 2 ships every flag
  `false`") that stopped being true long ago

What remains is the mirror itself, which is human-owned and cross-repo.

**Owner**: human (cross-repo). The contract side is done.

### 3. The verification sheets have never been run

`docs/test-specs/001`, `002`, `003` are all `未実施` — every row. Under
baseline §22 the sheets are written by the agent and executed by a
person, and nothing may be called complete until a person has run it.
v1.0 is the strongest "complete" claim the project can make, so shipping
it against three untouched sheets contradicts the rule directly.

Sheet 001 is partly answerable right now: the Firestore emulator is up,
which makes rows 2–9 runnable. Rows 1, 10 and 11 stay `未実施` until the
matching environments exist. Teardown when done:

```sh
docker compose -f docker/firestore-emulator/compose.yaml down
```

**Owner**: human only. The agent must not write `OK` into these files.

### 4. Code signing — or an honest note that there is none — **CLOSED 2026-08-16**

**Resolved via the alternative path.** The maintainer decided not to buy
certificates; ADR-0106 records that as a decision rather than a deferral,
and the disclosure now sits on the download page, in both README sections,
and prepended to every release body by `release.yml`. `site/page.test.mjs`
fails if the "not yet / planned / follow-up" phrasing returns. Three gates
remain, all human-owned.

The original statement of the gate follows.


Unsigned artifacts trip SmartScreen on Windows and Gatekeeper on macOS.
The binary is already handed to someone outside the project, so the
warning is not hypothetical. `release.yml` carries commented `codesign` /
`notarytool` / `stapler` placeholders (ADR-0044 §Future); what is missing
is paid certificates and the repo secrets to hold them (secrets are
human-only, baseline §15).

This one is a purchase decision, so it has a defined alternative: **if
the certificates are not bought, say so in `README.md` and in the 1.0
release notes as a known limitation, and ship anyway.** Shipping unsigned
is defensible. Shipping unsigned without saying so is not.

**Owner**: human (purchase, then secrets).

## Deliberately not gates

**Bookkeeping, already corrected on 2026-08-16.** Phase 2 was still
labelled `*(current)*` even though every item was `[x]` and its exit
criterion referenced `crates/dbboard-ui`, a crate ADR-0089 deleted.
`Export results (CSV / JSON)` was unchecked although ADR-0035 shipped
CSV/TSV; only the JSON format is genuinely missing, and it is now its own
line. Both read as open work and were not.

**Post-1.0 features.** Saved queries, schema diff between connections,
JSON export, Linux packaging, cold-start under 1s, and everything in
Phase 7+. None of them touch the contract, so all of them fit in 1.x. If
any of these were a gate, 1.0 would never arrive — which is the failure
mode this list exists to avoid.

## Done when

- [ ] #161 closed, **or** the behaviour and its Ctrl+Enter workaround
      written into `README.md` and the 1.0 release notes
- [ ] contract mirrored to `dbboard-web`
- [ ] sheets 001–003 executed by a person, to the extent their
      environments allow, with the untestable rows left `未実施`
- [x] signed artifacts, **or** the unsigned state written into `README.md`
      and the release notes — *taken the second way, ADR-0106 (2026-08-16)*
