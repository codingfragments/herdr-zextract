# herdr-zextract — agent conventions (Claude Code, pi, …)

Mirrors the gitflow used in the original `zellij-zextract` / `zellij-flash`
repos, adapted for this port's binary-release model.

## Gitflow

- **Always create a branch before making any change or commit** — even one-line fixes.
- Branch prefixes:
  - `bug/` — bug fixes
  - `feature/` — new features
  - `phase/` — larger milestone / multi-commit work (e.g. each item in
    PLANNING.md §11 Implementation phases)
  - `release/<version>` — release prep (version bump + CHANGELOG)
- If the type is unclear, ask before creating the branch.
- Every branch lands via PR to `main` — no direct commits to `main`.
- Stay on the working branch until the PR is explicitly merged; switch back
  to `main` only after merge.

## Workflow

- Commit frequently within a branch as work progresses.
- Do not push without user approval — summarise what's testable and wait
  for "push" / "looks good".
- End each phase with: what to test, how to trigger it, what works vs
  what's still a stub.
- **Current work: [rex-migration.md](rex-migration.md)** — make this one
  binary serve both Herdr and Rex (Host trait, Rex backend, Lua launcher,
  mise distribution). Read it first, work its phases in order (one
  `phase/rex-N-…` branch/PR each), and update its Status lines, checkboxes
  and spike table as you go. Herdr behavior must not regress.
- PLANNING.md §11 phases (the original herdr port) are complete; PLANNING.md
  remains the design reference for matchers, picker and config.

## Release process

Two-step merge flow, same shape as the original Zellij plugins:

1. **Code PRs** (`bug/`, `feature/`, `phase/`) — code changes only; merge
   first.
2. **Release PR** (`release/<x.y.z>`) — separate branch/PR containing:
   - `Cargo.toml` version bump (semver: patch/minor/major)
   - `CHANGELOG.md` entry
3. Merge the release PR to `main`.
4. Tag the resulting merge commit: `git tag v<x.y.z> && git push origin v<x.y.z>`
5. Pushing the tag triggers `.github/workflows/release.yml`, which builds
   release binaries for all target triples (see PLANNING.md §9), computes
   SHA-256 checksums, and publishes the GitHub release automatically.

> Never push a `v*.*.*` tag from a feature branch or before the release PR
> is merged.

## Project conventions

- Target platforms: macOS (arm64 + x86_64) and Linux (x86_64 + aarch64) —
  no Windows support planned.
- Versioning follows semver.
- Once a manifest exists, plugin registration/config lives in Herdr's own
  config, not this repo — this repo owns the binary and its
  `herdr-plugin.toml` only.
