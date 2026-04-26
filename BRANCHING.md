# Branching & Upstream Sync Strategy

このリポジトリ (`suisya-systems/renga`) は [`Shin-sibainu/ccmux`](https://github.com/Shin-sibainu/ccmux) のフォークです。
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

- **Git tag**: 普通の semver (`vX.Y.Z`)。v0.5.7-fork.1 〜 v0.5.7-fork.3 では `-fork.N` 接尾辞で prerelease 扱いしていたが、フォークは npm パッケージ名 (旧 `ccmux-fork`、現在 `@suisya-systems/renga`) と GitHub リポジトリ名で既にアイデンティティが確保されており、suffix は誤った "pre" 信号をユーザーに送るだけだった (v0.5.7-fork.3 リリース後に見直し)。今後は通常の semver で切る。
- **Prerelease が必要な場合** (大規模変更の先行公開など): `vX.Y.Z-rc.N` / `-beta.N` 等を使う。`contains(github.ref_name, '-')` で workflow が自動的に prerelease 扱い + npm dist-tag `next` に振り分ける (`.github/workflows/release.yml`)
- **npm パッケージ名**: 現在は `@suisya-systems/renga` (scoped)。元々 `ccmux-fork` で publish しており、Issue #102 / PR #152・#153 の流れで `@suisya-systems/renga` に rename した (PR #152 で一旦 `renga-fork` に書き換えたが publish 前に PR #153 で scope 付き名に再変更したため、npm 上に存在する旧名は `ccmux-fork` のみ)。`renga` (unscoped) は他者が先行 publish 済みのため scope 付きで確保した
- 上流 (`Shin-sibainu/ccmux`) は `0.5.x` 系を別途リリースしているが、こちらの fork はそれとは独立に semver を進める。バージョン番号の同期は取らない

## ブランチ保護

GitHub 側で以下を設定:

- `main`: PR 必須 / CI 必須 / force-push 禁止 / 直 push 禁止
- `master`: 管理者のみ push 可 (上流同期専用)

## 関連

- Claude Code で上流同期作業をする時は `.claude/skills/upstream-sync/` Skill が自動で発動する
- リリース手順の詳細は `CLAUDE.md` の Release Process を参照
