# 0036: a connection whose SSH key moved offers only "reconnect", which can never work

- **Status**: open
- **Phase**: 1
- **Opened**: 2026-09-24

## Context

Found by launching the app on 2026-09-24, not by a test.

A connection that tunnels over SSH failed on this machine with:

```
connection failed: ssh tunnel: failed to load ssh private key
  '<a Windows path from the old machine>': No such file or directory (os error 2)
```

The path is an **absolute Windows path left over from the machine this
workspace was migrated from** (2026-08-26, ADR-0119 era). Counting lines in
`connections.toml` that carry a Windows-shaped path or a `.ppk` suffix:
**three**. The real paths are PII under ADR-0055 and stay out of this file.

**The error text is good.** It names the exact file, so the cause is
obvious to anyone who reads it.

**What is offered is not.** The error banner's only action is *reconnect*
(`apps/desktop/src/routes/+page.svelte:277`), and the pill beside the
connection name offers the same one (`:252`). For a key file that does not
exist, **reconnecting fails identically every time** — the state it retries
is not the state that is wrong. There is no path from the error to the form
that would fix it.

So the app puts the answer on screen and then offers only the one button
that cannot use it.

## A second thing, smaller

The key in question is a `.ppk`. The add/edit form already says these are
not read:

> PuTTY .ppk files are not read — export the key in OpenSSH format first.
> (`apps/desktop/src/lib/i18n/messages.ts:316`)

**That sentence lives only in the form's help text**, where it is read
before the mistake and not after it. This connection would have failed on
format even with the path repaired, and nothing at failure time would have
said so.

Not the main bug — the file is missing, so format never came up — but the
same shape: the app knows something useful and says it in the one place the
person is not looking.

## Acceptance

- [ ] A failing test covers a connection whose key path does not resolve
- [ ] The error surface offers a way to **edit the connection**, not only to
      retry it. v0.11.0 shipped connection repair and duplication — check
      whether that flow already exists and is simply not reachable from here
      before building anything new
- [ ] A key path that does not resolve is distinguishable, in what the person
      sees, from a key that resolves but cannot be read
- [ ] A `.ppk` path says so **at failure time**, not only in the form
- [ ] Decide whether migration deserves more than per-connection repair —
      three connections carry stale paths, and fixing them one error at a
      time means hitting the same wall three times

## Notes

**Do not put the real paths in this repository** (ADR-0055). They carry a
business identifier, a host address and an account name. The mapping lives
in the untracked place, same as the connection names.

Whether the `.ppk` question matters depends on what those three connections
actually point at; two of them have not been opened on this machine, so
nothing is known about them beyond the shape of the path.

Related: [ADR-0055](../../docs/decisions.md) (PII), the migration section at
the top of [`next-actions.md`](../next-actions.md).
