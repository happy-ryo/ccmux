---
name: upstream-sync
description: Use when syncing this fork with Shin-sibainu/ccmux upstream, merging upstream changes into main, or sending a PR back to upstream. Triggers on phrases like "上流を取り込む", "upstream sync", "フォーク元と同期", "上流に PR", "sync fork".
---

# Upstream Sync Skill

このリポジトリは `Shin-sibainu/ccmux` のフォーク (`happy-ryo/ccmux`)。
上流同期と逆 PR の正しい手順は `BRANCHING.md` が正本。この Skill はその要約。

## ブランチ役割 (要点)

- `main` = 独自開発の本流 (default / リリース対象)
- `master` = `upstream/master` のミラー専用 (FF のみ)
- 通常ブランチは `main` から切って `main` に PR
- 上流への PR は `master` から切って cherry-pick

## 前提確認

最初に以下を確認する:

```bash
git remote -v | grep upstream
```

`upstream` remote が無ければ:

```bash
git remote add upstream https://github.com/Shin-sibainu/ccmux.git
git fetch upstream
```

## A. 上流を main に取り込む

```bash
git fetch upstream

# master を upstream/master に FF 追従
git checkout master
git merge --ff-only upstream/master
git push origin master

# main に取り込む sync PR を作成
git checkout main
git pull
git checkout -b chore/sync-upstream-$(date +%Y%m%d)
git merge master     # コンフリクトは解消する
git push -u origin HEAD
gh pr create --base main --title "chore: sync upstream $(date +%Y-%m-%d)"
```

**rebase ではなく merge**。`main` は公開タグ済みで履歴改変禁止。

## B. 上流に PR を返す

`main` から直接 PR しない (独自コミットが混入する)。必ず `master` ベースで新ブランチを切り、必要な commit だけ cherry-pick する。

```bash
git fetch upstream
git checkout master
git merge --ff-only upstream/master
git checkout -b upstream-pr/<topic>
git cherry-pick <sha>...          # main の該当コミット
git push -u origin HEAD
gh pr create --repo Shin-sibainu/ccmux --base master
```

## 禁止事項

- `main` を force-push しない
- `main` から上流へ直接 PR しない
- `master` に独自コミットを追加しない (常に upstream ミラー)
- 上流取り込みを rebase で行わない
