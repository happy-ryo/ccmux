---
name: upstream-sync
description: Use ONLY when the user explicitly asks to cherry-pick a specific commit from the Shin-sibainu/ccmux upstream, or to send a one-off PR back to upstream. renga is an independent project — there is no scheduled or periodic sync. Triggers on phrases like "上流から cherry-pick", "upstream の <commit> を取り込む", "上流に PR を返す", "cherry-pick from upstream", "send PR to upstream ccmux". Do NOT trigger on generic "fork sync" / "週次 sync" requests — those are no longer policy.
---

# Upstream Cherry-pick Skill

renga (`suisya-systems/renga`) は [`Shin-sibainu/ccmux`](https://github.com/Shin-sibainu/ccmux) から派生したが、**現在は独立した本流として開発している**。
定期的な上流 sync / 週次マージは行わない。`BRANCHING.md` の divergence policy が正本で、この Skill はその ad-hoc な cherry-pick と逆 PR 手順だけを補助する。

## いつ使うか / 使わないか

- ✅ 使う: ユーザーから「upstream のこの commit を取り込んで」「この修正を上流に PR して返して」といった**具体的な単発指示**を受けたとき
- ❌ 使わない: 「上流と同期して」「最近の upstream を取り込んで」といった包括的な依頼 — まずユーザーに「renga は独立 main で運用しており包括 sync はしない」旨を伝え、本当に欲しい個別 commit を確認する
- ❌ 使わない: ユーザーから明示の指示がないのに「上流に還元しよう」と提案する (root `CLAUDE.md` のポリシーに反する)

## ブランチ役割 (要点)

- `main` = renga の本流 (default / リリース対象)。上流とは独立に進化
- `master` = `upstream/master` を必要なときだけ FF 追従するスナップショット (記録 / cherry-pick ベース用)。独自 commit は乗せない
- 通常の機能ブランチは `main` から切って `main` に PR
- 上流還元用ブランチ (`upstream-pr/*`) は `master` から切る

詳細は `BRANCHING.md` を参照。

## 前提: upstream remote の追加 (任意・未追加なら一度だけ)

```bash
git remote -v | grep upstream || \
  git remote add upstream https://github.com/Shin-sibainu/ccmux.git
git fetch upstream
```

renga の通常開発に upstream remote は不要。cherry-pick / 逆 PR をするときに初めて追加する。

## A. 上流の特定 commit を main に cherry-pick する

包括 sync ではなく、**ユーザーが指定した個別 commit (またはごく少数)** を取り込む手順。

```bash
git fetch upstream

# 1. master を upstream/master に FF 追従 (スナップショット更新、任意)
git checkout master
git merge --ff-only upstream/master
git push origin master

# 2. main から作業ブランチを切って cherry-pick
git checkout main
git pull
git checkout -b chore/cherry-pick-<topic>
git cherry-pick <upstream-sha> [<upstream-sha> ...]
# コンフリクトが大きい場合は cherry-pick を中断し、renga 流で書き直すことを検討
cargo build --release && cargo test
git push -u origin HEAD
gh pr create --base main --title "chore: cherry-pick <topic> from upstream ccmux"
```

注意点:
- **rebase ではなく cherry-pick**。`main` は公開タグ済みで履歴改変禁止
- renga 側との実装乖離が大きいので、コンフリクトが激しい場合は cherry-pick を諦めて renga 流で再実装する方が早いことが多い
- 取り込んだ upstream commit の SHA を PR description に書く (後追いトレース用)

## B. 上流に PR を返す (ユーザー明示指示時のみ)

`main` から直接 upstream に PR を出さない — renga 独自の commit が混入する。必ず `master` ベースで新ブランチを切り、必要な commit だけ cherry-pick する。

```bash
git fetch upstream
git checkout master
git merge --ff-only upstream/master
git checkout -b upstream-pr/<topic>
git cherry-pick <main-side-sha> [<main-side-sha> ...]
# renga 固有の依存 / 命名 (renga, @suisya-systems/renga 等) が混入していないか必ず確認
git push -u origin HEAD
gh pr create --repo Shin-sibainu/ccmux --base master
```

ユーザーから明示の指示がない限り、上流 PR は提案も実行もしない (root `CLAUDE.md` の Fork & Branching ポリシー)。

## 禁止事項

- `main` を force-push しない
- `main` から upstream へ直接 PR しない
- `master` に独自 commit を追加しない (常に upstream のミラー)
- 上流取り込みを rebase で行わない
- 包括的 / 定期的 sync を勝手に行わない (divergence policy に反する)
