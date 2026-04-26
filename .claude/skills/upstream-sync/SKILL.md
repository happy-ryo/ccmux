---
name: upstream-sync
description: Use when the user explicitly asks to cherry-pick a specific commit from the Shin-sibainu/ccmux upstream into renga's main, or to send a one-off PR from renga back to that upstream. This is an ad-hoc, per-commit workflow — renga is an independent project and does not do periodic upstream merges. Triggers on phrases like "cherry-pick from upstream", "上流から cherry-pick", "send PR to upstream ccmux", "上流に PR を返す", "pull commit X from Shin-sibainu/ccmux", "upstream の <commit> を取り込む".
---

# Upstream Cherry-pick Skill

renga (`suisya-systems/renga`) was forked from [`Shin-sibainu/ccmux`](https://github.com/Shin-sibainu/ccmux), but is now developed as an independent main line. `BRANCHING.md` is the source of truth for branch policy; this skill only assists with the two ad-hoc workflows that need an actual procedure: pulling a single upstream commit into `main`, and sending a one-off reverse PR back to upstream.

## When this skill should fire — and when it should NOT

✅ Fire when the user names a concrete operation:
- "Cherry-pick upstream commit `<sha>` into main."
- "Send this fix back to upstream as a PR."
- "Update `master` to `upstream/master` so I can cherry-pick from it."

❌ Do NOT fire — and instead push back to the user — when the request is generic or periodic:
- "Sync the fork with upstream", "merge in the latest upstream", "do the weekly upstream sync", anything implying a blanket merge.
  - renga has no scheduled or periodic sync. Tell the user that, and ask which specific upstream commit(s) they actually want.
- "Open a PR to upstream for our recent improvements" without an explicit go-ahead.
  - Per root `CLAUDE.md`, do not propose or open reverse PRs unless the user explicitly asks for one.

## Branch roles (summary)

- `main` — renga's mainline (default branch, release target). Evolves independently of upstream.
- `master` — snapshot mirror of `upstream/master`, kept around as a base for cherry-picks and as a record. Carries no renga-specific commits.
- Feature branches — branched from `main`, PR'd into `main`.
- `upstream-pr/*` — branched from `master` for reverse PRs only.

See `BRANCHING.md` for the full policy.

## Prerequisite: add the upstream remote (one-time, optional)

renga's normal development does not need the upstream remote. Add it only when this skill is invoked:

```bash
git remote -v | grep upstream || \
  git remote add upstream https://github.com/Shin-sibainu/ccmux.git
git fetch upstream
```

## A. Cherry-pick a specific upstream commit into `main`

Used when the user has named one (or a small number of) upstream commits to bring into renga.

```bash
git fetch upstream

# 1. Update the master snapshot to upstream/master (recommended but optional).
git checkout master
git merge --ff-only upstream/master
git push origin master
```

If the FF merge fails because upstream rebased `master`, this is the one sanctioned case for force-pushing `master` (see `BRANCHING.md` §"ブランチ構成"):

```bash
git fetch upstream
git checkout master
git reset --hard upstream/master
git push --force-with-lease origin master
```

`master` is a pure mirror, so a forced reset is safe — never do this on `main`.

```bash
# 2. Branch off main and cherry-pick the requested commit(s).
git checkout main
git pull
git checkout -b chore/cherry-pick-<topic>
git cherry-pick <upstream-sha> [<upstream-sha> ...]
# If conflicts are large, abort and reimplement in renga's own style instead.
cargo build --release && cargo test
git push -u origin HEAD
gh pr create --base main --title "chore: cherry-pick <topic> from upstream ccmux"
```

Notes:
- Use `cherry-pick`, never `rebase` or a blanket merge of `master` into `main`. `main` carries published tags and must not have its history rewritten.
- Record the source upstream SHA(s) in the PR description so the provenance is traceable later.
- renga's implementation has diverged enough that conflicts often dominate the cherry-pick. When that happens, abandon the cherry-pick and reimplement the change the renga way — that is normally faster and cleaner.

## B. Send a reverse PR to upstream (only on explicit user request)

Never PR upstream from `main` directly — renga-specific commits would leak in. Always branch from `master` and cherry-pick exactly the commits you want to upstream.

```bash
git fetch upstream
git checkout master
git merge --ff-only upstream/master    # or the force-with-lease recovery above if upstream rebased
git checkout -b upstream-pr/<topic>
git cherry-pick <main-side-sha> [<main-side-sha> ...]
# Audit the diff for renga-specific identifiers (renga, @suisya-systems/renga, renga-only files, etc.)
# and remove or rewrite them before pushing.
git push -u origin HEAD
gh pr create --repo Shin-sibainu/ccmux --base master
```

## Forbidden

- Force-pushing `main`.
- Opening PRs against upstream from `main` directly.
- Adding renga-specific commits to `master` (it must stay a pure mirror of `upstream/master`).
- Pulling upstream into `main` via `rebase` or via a blanket `git merge upstream/master`.
- Performing a periodic / blanket "fork sync" without an explicit, per-commit user instruction.
