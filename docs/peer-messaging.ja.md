# Claude Code と Codex ペイン間のメッセージング

*Language: [English](./peer-messaging.md) / 日本語*

同じ renga タブに並べた Claude Code と Codex のペイン同士が、`renga-peers` MCP サーバ経由でメッセージを送り合えるようになります。片方のエージェントに「これを調べておいて」と頼んだり、失敗したテストの原因追いを引き継いだりを、ユーザーが手で中継しなくても進められます。Claude は `<channel source="renga-peers">` タグで受け取り、Codex には renga が PTY 経由で `check_messages` を促す nudge を送り、実本文は MCP inbox から読みます。

本ページは **運用ワークフロー** を扱います — セットアップ、起動、2 ペイン例、トラブルシュート。**canonical な MCP ツール一覧、パラメータスキーマ、エラーコード、frozen-prefix 文字列**は [`api-surface-v1.0.md`](./api-surface-v1.0.md) §1 (英語のみ) にあります。本ページではそのコントラクトを再掲しません。

> **[`claude-peers-mcp`](https://github.com/happy-ryo/claude-peers-mcp) との違い** — 両者ともツール表面はほぼ同じですが、`claude-peers-mcp` は `cwd` / `git_root` / `PID` からスコープを推測 (ヒューリスティック、衝突しうる) します。renga-peers は **renga タブ** を権威スコープとして使う — ユーザーが文字通り同じタブに置いたペイン群が対象です。両者は同じ Claude install 内で共存できます (チャンネル名が衝突しない: `server:renga-peers` vs `server:claude-peers`)。

## セットアップ (1 回だけ)

```bash
renga mcp install --client claude
renga mcp install --client codex   # Codex peer も使う場合
```

いま走っている `renga` バイナリを `renga-peers` という名前で、選択した client のユーザー設定に登録します。同じ登録の重複実行は冪等で、renga アップグレード後に上書きしたいときは `--force` を付けてください。`renga mcp uninstall --client …` と `renga mcp status --client …` がそれぞれ逆操作・状態確認です。

Codex では、既定の install は client CLI の正規登録経路を尊重しつつ、peer messaging に必要な最小限の `env_vars` passthrough だけを補正します。`check_messages` と `send_message` も auto-approve 寄りにしたい場合は、明示的に opt-in してください:

```bash
renga mcp install --client codex --codex-auto-approve-peer-tools
```

このフラグは `send_keys` や pane 操作系のような、より強いツールまでは自動承認しません。

## peer チャネル付きで起動する

配送方式は client ごとに違います。

- **Claude Code** は MCP の experimental channel 機能を使うので、起動時に毎回 `--dangerously-load-development-channels server:renga-peers` が必要です。
- **Codex** は `renga mcp install --client codex` で入れた MCP 登録を使います。これが入っていれば plain `codex` 起動で足ります。非フォーカスの worker pane が落ち着いたら renga が `check_messages` を促す nudge を送り、実際の peer 本文は `check_messages` で読みます。対象の Codex pane がフォーカス中なら、PTY 注入を即座にせずローカル通知 overlay を表示します。

Claude の起動フラグを毎回手で打たなくて済むように、renga 側から 2 つの経路を用意しています:

- **`Alt+P`** — フォーカス中のペインに `claude --dangerously-load-development-channels server:renga-peers ` を入力 (末尾にスペース、**Enter は押されない**)。そのまま Enter で起動してもいいし、追加引数を続けて書いてから Enter でも OK。シェルの種類を問わず動作します。
- **`renga split --role claude`** / **`renga new-tab --role claude`** — 新しいペインを開いて、上記フラグ付きの Claude Code を自動起動。`--command "..."` を明示したらそちらが優先されるので、カスタム起動の逃げ道は残ります。

Codex を会話の中から増やしたい場合は `spawn_codex_pane(direction, …)` を使います。

## 2 ペインでのやり取り

```
タブ A                         タブ B (独立)
┌──────────┬──────────┐        ┌──────────┐
│ claude-1 │ claude-2 │        │ claude-3 │
│          │          │        │          │
│  peers ──┼──▶ ✓     │        │  peers   │  ← claude-1/2 は見えない
│  send ◀──┼── msg    │        │          │
└──────────┴──────────┘        └──────────┘
```

Claude A の会話で:

```
> list_peers を呼んで
# → id=2 (同じタブにいる相方) が返ってくる

> send_message を to_id=2, message="src/app.rs の handle_split を読んで要約して" で呼んで
```

Claude B の次のターンのコンテキストに `<channel source="renga-peers">src/app.rs の handle_split を読んで…</channel>` タグで届き、Claude B は「ユーザーではなく相方からの依頼」と判別 (タグの `source` 属性が決め手) して要件を処理 → 同じ `send_message` で返信します。

安定 name 解決があるので、orchestrator は数値 id を追いかけずに `"secretary"` / `"worker-1"` で peer を指せます。途中で名前を付け替えたい場合は `set_pane_identity` を使います。push される本文には `📡 PEER MESSAGE … NOT FROM USER` バナーが付くので、トランスクリプトを眺める運用者から見ても「`Human:` 風に表示されているターンが peer 由来か user 由来か」を一目で見分けられます。同一本文の数秒以内の連投はサーバ側で 1 通に畳まれるので、トランスクリプトに幻の重複ターンが現れません ([renga#221](https://github.com/suisya-systems/renga/issues/221))。

## ペイン操作を組み合わせる

ワーカーが対話プロンプトで止まった場合も、オーケストレータは会話の中で完結できます:

- `inspect_pane(target="worker-1", lines=20)` でワーカー自身に画面状態を語らせずにスナップショット
- `send_keys(target="worker-1", text="y", enter=true)` (もしくは `Esc`、矢印、`Ctrl+C` のような名前付きキー) でプロンプトに応答
- `poll_events` の cursor をターン間で持ち回ると、毎回タブ全体を `list_panes` し直さずに `pane_started` / `pane_exited` を追える

ペイン操作系ツール (`list_panes` / `spawn_pane` / `spawn_claude_pane` / `spawn_codex_pane` / `close_pane` / `focus_pane` / `new_tab` / `inspect_pane` / `send_keys` / `set_pane_identity` / `poll_events`) でオーケストレータが必要とする表面はほぼ揃います。各ツールのパラメータスキーマ、返り値の形、エラーコードの完全な一覧は [`api-surface-v1.0.md`](./api-surface-v1.0.md) §1 (英語) を参照してください。

> **`claude` 自動アップグレード。** `spawn_pane` / `new_tab` / `renga split` / `renga new-tab`、および layout TOML の `command = "claude"` 指定は、peer 対応の起動コマンドに自動で書き換えられます。各呼び出し側で `--dangerously-load-development-channels server:renga-peers` を覚えていなくても、新ペインが renga-peers ネットワークに参加します。orchestrator が Claude を起動したい場合は `spawn_pane(command="claude ...")` より `spawn_claude_pane` を推奨 — launch policy が renga 側に集約され、`args[]` に予約済みフラグが混入したら `invalid-params` で拒否されます。

> **ペインの `cwd`。** `spawn_pane` / `new_tab` / `renga split --cwd` / `renga new-tab --cwd` / layout TOML `cwd = "..."` で新ペインの作業ディレクトリを指定できます。絶対パスはそのまま、相対パスは呼び出し元ペインの cwd (MCP)、シェルの cwd (CLI)、renga プロセスの cwd (layout TOML) を基準に解決されます。存在しないパスはレイアウト変更前に `cwd_invalid` で失敗するため、half-mutated なレイアウトになりません。`command` に `cd <dir> && ...` を書くと `claude` 自動アップグレードが効かなくなるので、`cwd` フィールドで指定するのが推奨です。

## うまく動かないとき

- **`list_peers` が "renga not reachable from this peer client" を返す** — client が renga の外で起動されたか、renga ペインの環境変数を引き継げていません。renga のペイン内から起動し直してください（Claude は `Alt+P` / `renga split --role claude`、Codex は `renga mcp install --client codex` 後の plain `codex` または `spawn_codex_pane`）。
- **相手に送ったメッセージが `<channel>` タグで表示されない** — 起動時のフラグ `--dangerously-load-development-channels server:renga-peers` を付け忘れています。`claude` と打つ代わりに `Alt+P` を使えばフラグ付きのコマンドが挿入されるので事故りにくくなります。
- **Codex に送ったのに反応がない** — renga は Codex ペインが PTY 入力を安全に受けられる状態で、かつ非フォーカスになってから `check_messages` を促す nudge を流し込みます。フォーカス中に届いたメッセージは、会話を汚さないように通知 overlay へ回します。`Alt+Enter` / `Ctrl+Enter` で `check_messages` を呼ぶための文面だけ挿入し、`Esc` なら無視、Enter を押して実行するかどうかは人間が決めます。フォーカスを外せば worker と同じ deferred nudge に戻ります。実際の依頼本文は MCP inbox 側にあり、`check_messages` の返り値が真実です。
- **新しい Codex pane で `check_messages` / `send_message` の承認がまた出る** — Codex の承認は pane-local に振る舞うことがあります。`renga mcp install --client codex --codex-auto-approve-peer-tools` で安全な peer messaging 系の承認を事前設定できますが、Codex のバージョンや実行形態によっては、新しい pane で一度だけ warm-up 承認が必要です。
- **`spawn_codex_pane` が `[codex_not_installed]` で失敗する** — Codex の MCP 設定 (`~/.codex/config.toml`) に renga-peers エントリがない、ファイルが読めない、もしくは `[mcp_servers.renga-peers.env]` に `RENGA_PEER_CLIENT_KIND=codex` が登録されていません。`renga mcp install --client codex` を 1 回実行してください。env 値だけが欠けた既存エントリも install 経路で self-heal します。
- **`send_keys` が効いていないように見える** — `send_keys` は target ペインの PTY に生の入力バイトを書き込むだけで、帯域外の「承認」操作ではありません。まず `inspect_pane(target=…, lines=20)` で本当に入力待ちか確認し、レイアウトが動く運用ではフォーカス推測ではなく安定した pane `name` を target に使ってください。
- **`poll_events` が想定より早く `events: []` を返す** — `types=[…]` フィルタは返却結果を絞るだけで、非一致イベントでも long-poll は解除されて `next_since` は前進します。返ってきた cursor でそのまま再 poll してください。`events_dropped` が来た場合だけ、1 回 `list_panes` で再同期すると安全です。
- **renga をアップグレードしたら繋がらなくなった** — 登録されているバイナリのパスが古いです。`renga mcp install --client claude --force` や `renga mcp install --client codex --force` で今の `renga` に更新してください。

## 関連ドキュメント

- [`api-surface-v1.0.md`](./api-surface-v1.0.md) — MCP ツール / パラメータ / 返り値 / エラーコードの canonical wire-frozen リスト (英語のみ)
- [`keymap.ja.md`](./keymap.ja.md) — フルキーバインド (`Alt+P` peer-launch と file-tree の `c` / `v` 分割起動を含む)
- [`configuration.ja.md`](./configuration.ja.md) — TOML 設定キー (MCP / ペイン操作の表面とは分離)
