# Branching & Upstream Sync Strategy

このリポジトリ (`happy-ryo/ccmux`) は [`Shin-sibainu/ccmux`](https://github.com/Shin-sibainu/ccmux) のフォークです。
上流の変更を取り込みつつ、独自の機能を先行して開発する方針を取っています。

## ブランチ構成

| ブランチ | 役割 | push 権限 |
|---|---|---|
| `main` | **独自開発の本流** (default branch、リリース対象) | PR のみ。force-push 禁止 |
| `master` | **上流ミラー専用**。`upstream/master` を FF で追従するだけ | 上流が rebase した時に備え force-push 許可 |
| `feat/*`, `fix/*`, `chore/*` | 通常の機能ブランチ。`main` から切る | PR で `main` にマージ |
| `upstream-pr/*` | 上流へ還元する PR 用。`master` から切る | `Shin-sibainu/ccmux` へ PR |

### なぜ `master` を残すのか

- 上流が `master` を使い続けている
- `upstream/master` と同名で運用するとミラー関係が直感的
- 将来上流が `main` にリネームしたら、こちらの `master` は削除して対応

## Remote

```bash
git remote add upstream https://github.com/Shin-sibainu/ccmux.git
git fetch upstream
```

## 日常運用

### 通常の機能開発

```bash
git checkout main
git pull
git checkout -b feat/xxx
# ... 実装 ...
gh pr create --base main
```

### 上流の変更を取り込む (週次 or 上流リリース時)

```bash
git fetch upstream

# 1. master を upstream/master に FF 追従
git checkout master
git merge --ff-only upstream/master
git push origin master

# 2. main に取り込むための sync ブランチを作成
git checkout main
git pull
git checkout -b chore/sync-upstream-YYYYMMDD
git merge master          # コンフリクト解消
gh pr create --base main --title "chore: sync upstream YYYY-MM-DD"
```

**rebase ではなく merge を使う**こと。`main` は公開タグ済みで履歴改変不可。マージコミットにより「いつ何を取り込んだか」が残る。

### 上流へ PR を返す

独自実装から汎用的に切り出せるものは、`master` を base にした別ブランチで送る。
`main` から直接 PR すると無関係の変更が混入するので厳禁。

```bash
git fetch upstream
git checkout master
git merge --ff-only upstream/master
git checkout -b upstream-pr/foo
git cherry-pick <main の commit>
git push origin upstream-pr/foo
gh pr create --repo Shin-sibainu/ccmux --base master
```

## リリース・npm パッケージ

- Git tag: フォーク識別子付き prerelease (`vX.Y.Z-fork.N` 等) を利用
- npm パッケージ名: 上流と衝突しないフォーク専用の名前を使用
- 具体的な移行作業は関連 Issue を参照

## ブランチ保護

GitHub 側で以下を設定:

- `main`: PR 必須 / CI 必須 / force-push 禁止 / 直 push 禁止
- `master`: 管理者のみ push 可 (上流同期専用)

## 関連

- Claude Code で上流同期作業をする時は `.claude/skills/upstream-sync/` Skill が自動で発動する
- リリース手順の詳細は `CLAUDE.md` の Release Process を参照
