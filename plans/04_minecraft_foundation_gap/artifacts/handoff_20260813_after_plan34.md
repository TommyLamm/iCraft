# Handoff after Plan34 — 2026-08-13

## Repository state

- Branch: `tommy-dev`.
- HEAD: this handoff commit (subject `docs: hand off final serial route gate`,
  parent `2ff7db8`); use `git rev-parse HEAD` for its immutable hash.
- `origin` is at `04f2069`; the branch is ahead by 5 commits.
- The branch has not been pushed. Do not push: explicit authorization for the
  external origin has not been granted.
- Recent commits, oldest to newest:
  - `9768262` — `fix(plan31): suppress replayed client ACKs`; the
    `GameplayResponseGate` suppresses cached ACKs that have already been
    presented.
  - `8336d99` — `test(plan32): align cached ACK acceptance`; Plan32 duplicate
    acceptance is aligned with the authority cache/client suppression behavior.
  - `a5bf6b3` — `fix(plan33): preserve newer fresh revisions`; fresh revision
    selection uses the supplied/high-water maximum.
  - `2ff7db8` — Plan34 container-break inventory conservation and atomic
    rollback.
  - Current handoff commit — `docs: hand off final serial route gate`; records
    the outstanding final gate.

Plans 31–34 are complete, with their respective commits and verification
artifacts. This handoff does not claim that the repository-wide final gate has
been completed, and it does not claim a full-suite result.

## Plan verification status

Plan34 focused verification passed:

- Plan34 debug: 4/4.
- Plan34 release: 4/4.
- Plan31 debug: 3/3.
- Plan31 release: 3/3.
- Authority mining focused regression: 1/1.
- Entity focused regression: 24/24.
- Save entity roundtrip: 1/1.
- Save entity persistence: 1/1.
- `cargo fmt --all -- --check`: pass.
- `git diff --check`: pass.

Plan33 corrective verification passed its focused checks: fresh-revision unit
1/1 in debug and release, legacy envelope 1/1 in debug and release, Plan33
topology 3/3 in debug and release, Plan30 regression 2/2 in debug and release,
plus formatting and diff checks. Do not infer a repository-wide pass from those
focused results.

## Dirty files and save-ID boundary

The following are user-owned dirty files. Do not stage, overwrite, or otherwise
discard their changes:

- `Cargo.toml`
- `assets/texture_atlas.png`
- `controls.config`

`src/entity.rs` and `src/save.rs` show `M` only because of CRLF/stat metadata
false-dirty behavior. Their `git diff` is empty and their worktree hashes equal
HEAD:

- `src/entity.rs`: `247aa011c0ffc0fa0ceac21ab551b556ef738051`
- `src/save.rs`: `cf92cdc70337045a759d84e87186470f452aabaa`

Do not commit these files. A subsequent agent may use
`git update-index --refresh` and re-check status, but must not use
`checkout`/`reset` to clear them.

## Next required work

The repository-wide serial final gate after Plan34 is still outstanding, and the
active goal remains active. After a context reset, the next agent must first call
`get_goal`, then work through CodeGraph/PowerShell with strictly serial
commands. At minimum, run these exact commands in order:

```text
cargo test --locked --no-fail-fast -- --test-threads=1
cargo test --locked --release --no-fail-fast -- --test-threads=1
cargo check --locked --release --all-targets
cargo fmt --all -- --check
git diff --check
git diff --cached --check
```

Then perform a completion audit across every numbered Plan and its README/artifact
claims. Keep status limited to the known user-owned dirty files (plus any
intentional handoff artifact), without staging or committing. If the full gate
fails, first attribute the failure to an existing Plan; create a new numbered
Plan only when a genuinely new gap is confirmed. Do not push to `origin` unless
the user explicitly authorizes it. Finally call `update_goal` with `complete`
only after all required work is actually finished.
