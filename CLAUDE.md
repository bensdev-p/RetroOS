# RetroOS — notes for Claude Code sessions

A retro-styled OS for the Raspberry Pi 5/4 (Debian trixie + our own Rust
compositor "Station", RetroDOS shell and RetroOS Desktop). Milestones 1–3 are
done; Milestone 4 is next.

**Read `docs/HANDOFF.md` first** — where things stand, the branch caveat,
the M4 plan, how to test, and how the user tests on hardware. Then
`docs/DESIGN.md` §18 and `docs/DECISIONS.md` as needed.

## Must-follow rules

- Before committing: `cargo fmt --all && cargo clippy --workspace --all-targets && cargo test --workspace`
  must be clean, and `tests/host/run-all.sh` must pass when Station or a
  GUI program changed (chain with `&&`, never `;`).
- Every deviation from `docs/DESIGN.md` gets a `docs/DECISIONS.md` entry
  (numbered; HANDOFF §5 says the next number).
- Hardware: add rows to `docs/HW_CHECKLIST.md` before asking the user for a
  run, record their results there and numbers in `docs/BUDGET.md`; never
  claim a hardware result the user did not report.
- Look at host-test screenshots (`out/test-host*/`), not just pass/fail.
- Commit `Cargo.lock` with new crates (the image build is `--locked`).
- Push to the session's designated branch; no pull request unless the user
  asks; no model names in the repo.
- Full images build in CI (arm64 runner, ~25 min), not locally.
