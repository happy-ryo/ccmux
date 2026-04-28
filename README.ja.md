# renga

*Language: [English](./README.md) / 日本語*

**複数の [Claude Code](https://docs.anthropic.com/en/docs/claude-code) と Codex エージェントを 1 つの TUI でオーケストレートするための AI ネイティブ・ターミナル基盤。mixed-client peer メッセージング、必要な部分だけ Claude 特化 UX、単一バイナリ。**

![renga スクリーンショット](screenshot.png)

## renga とは何か

renga は「ペイン自身が AI エージェントであることを知っている」ターミナルです。分割・タブ・フォーカスといった操作は他の TUI multiplexer と同じですが、内部ではそれぞれのペインを **第一級のエージェント・エンドポイント** として扱います。Claude Code が動いているペインは自動検出し、Codex ペインも同じ peer network に参加できるようにし、`spawn_claude_pane` / `spawn_codex_pane` / `set_pane_identity` / `new_tab` などのペイン制御ツールを提供します。peer のスコープは renga タブ単位で固定され、Claude は channel push、Codex は `check_messages` による pull 受信で協調します。各ペインは `role` ラベル（表示・フィルタ用途）も持ちますが、メッセージのルーティング自体は id / name 経由です。

主なユースケースは **エージェントのオーケストレーション** — 「窓口（secretary）」ペインがタスクを「ワーカー」ペインに振り分ける構成、サブエージェントを別ペインで並走比較する構成、長時間セッションが軽い調べ物だけ別ペインに投げる構成、Claude と Codex を同じタブで役割分担させる構成、などです。エージェントを常に 1 つしか起動しないなら、renga が今のターミナルより優位な点は限られます。複数同時に動かすなら、peer チャネルと AI 認識ペインモデルがそのまま価値になります。

### tmux / zellij との位置づけ

| | tmux / zellij | renga |
|---|---|---|
| ペインの抽象 | 汎用シェルセッション | **AI エージェント・エンドポイント**（安定 id / role / フォーカスフラグ付き） |
| ペイン間メッセージング | コピペ、手動 `send-keys`、外部 glue | 組み込み MCP `renga-peers` network。Claude は channel push、Codex は `check_messages` で pull 受信 |
| エージェントペイン起動 | ユーザーがクライアントごとの起動コマンドやフラグを手で管理 | `spawn_claude_pane` / `spawn_codex_pane` MCP ツール、Claude 用 `Alt+P` |
| IME / 日本語入力 | ホストターミナル任せ。Claude のストリーミング出力で候補窓が踊りがち | 専用 IME 合成 overlay。freeze-on-overlay + 周期 catch-up でキャレット直下に候補窓を固定 |
| 設定の表面積 | シェル glue / プラグイン / keytable | 小さな TUI バイナリ 1 本。レイアウト TOML でペイン構成と role を直接宣言 |

**やらないこと（non-goals）。** renga は **汎用 tmux 代替を目指していません**。tmux のセッション永続化、ネスト server モデル、プラグインエコシステム、スクリプタブルな自動化 API を網羅する意図はありません。ターミナルエミュレータでもありません（自前のフォント・グリフ描画は持たず、既存ターミナル上で動きます）。IDE プラグインでもチャット UI でもありません。狙いは狭く一点で、**「複数の Claude Code エージェントが 1 つのウィンドウで協調する」基盤として最良であること**、そして約 10 MB の単一バイナリで配布できるサイズに収めることです。

### 例: 窓口（secretary）+ ワーカー型オーケストレーション

[`claude-org`](https://github.com/suisya-systems/claude-org) / [`claude-org-ja`](https://github.com/suisya-systems/claude-org-ja) で実際に使われているレイアウト。窓口役の "secretary" ペインが renga-peers 経由でワーカーペインにタスクを振り分けます。

```
tab "project-X"
┌────────────────────┬────────────────────┐
│ secretary          │ worker-1           │
│ (claude, role=     │ (claude, role=     │
│  "secretary")      │  "worker")         │
│                    │                    │
│  send_message ────▶│  <channel ...> で  │
│   to_id="worker-1" │  受信              │
│                    │                    │
│◀── 返信 ───────────│                    │
└────────────────────┴────────────────────┘
```

secretary のチャットからワーカーを増やしてタスクを投げるのは MCP コール 2 回で完結します。シェルもコピペも不要です。

```
> call spawn_claude_pane with direction="right", role="worker", name="worker-1"
# 新ペインが現れ、peer チャネル配線済みで Claude が起動する

> call send_message with to_id="worker-1" and
  message="please grep src/ for TODO(perf) and report file:line + 1 line of context"
```

worker-1 は次のターンで `<channel source="renga-peers" from_id="…" from_name="secretary">…</channel>` タグを受け取り、ユーザー入力ではなく peer リクエストだと判断し（`source` 属性で見分けられる）、作業して `send_message(to_id="secretary", …)` で返信します。安定 name 解決があるので、secretary は数値 id を追いかけずに `"secretary"` / `"worker-1"` で peer を指せます。途中で名前を付け替えたい場合は `set_pane_identity` で行えます。

ワーカーが対話プロンプトで止まった場合も、オーケストレータは会話の中で完結できます。まず `inspect_pane(target="worker-1", lines=20)` で画面状態を確認し、そのうえで `send_keys(target="worker-1", text="y", enter=true)` や `Esc` / 矢印 / `Ctrl+C` などの名前付きキーで応答します。ワーカーが増減する運用では `poll_events` の cursor をターン間で持ち回ると、毎回タブ全体を `list_panes` し直さずに `pane_started` / `pane_exited` を追えます。

同じ仕組みでより複雑なレイアウト（1 タブに dispatcher + 複数ワーカー / 評価者ペインがワーカー出力を監視 / 隣のタブに独立したチームをもう 1 セット …）にもスケールします。peer メッセージングは **タブ単位でスコープ** されており、`new_tab` はレイアウトを広げる手段でチャネルを横断するものではありません。ツール一覧は [Claude Code と Codex ペイン間のメッセージング](#claude-code-と-codex-ペイン間のメッセージング) を参照してください。

## できること

- **ペイン分割** — 縦横に分割、各ペインは独立したシェル (PTY)
- **タブ** — プロジェクトごとに独立したワークスペースをタブで切替
- **mixed-client peer メッセージング** — 同じタブに並べた Claude Code と Codex が `renga-peers` で協調できる。Claude は `<channel source="renga-peers">` タグで push 受信し、Codex は `check_messages` で poll する ([詳細](#claude-code-と-codex-ペイン間のメッセージング))
- **ファイルツリー** — アイコン付きのサイドバー、ディレクトリは展開/折りたたみ
- **プレビュー** — シンタックスハイライト付き、画像ファイルも表示 (Sixel / Kitty / iTerm2 / halfblocks 自動選択)
- **Claude Code の自動検出** — Claude Code が動いているペインは枠がオレンジになる
- **`cd` に追従** — ディレクトリを移動するとファイルツリーとタブ名も自動で切り替わる
- **マウス操作** — クリックでフォーカス、境界ドラッグでリサイズ、ホイールで履歴スクロール
- **10,000 行のスクロールバック** (ペインごと)
- **ダークテーマ** (Claude 風のカラースキーム)
- **Windows / macOS / Linux** 対応、単一バイナリ (約 8〜10 MB、プラットフォーム依存、追加ランタイム不要)

## インストール

### npm (おすすめ)

```bash
npm install -g @suisya-systems/renga
```

**最新版へのアップデート:**

```bash
npm update -g @suisya-systems/renga
npm install -g @suisya-systems/renga@latest
```

npm が新しくキャッシュも素直なら `npm update -g` で上がります。スキップされたように見える場合は `@latest` 付きの `install` で確実に最新を引いてください。`renga --version` で確認し、[最新リリース](https://github.com/suisya-systems/renga/releases/latest) と照合できます。

> 旧 `ccmux-fork` を入れている場合は: `npm uninstall -g ccmux-fork && npm install -g @suisya-systems/renga`
>
> 上流の `ccmux-cli` を入れている場合は: `npm uninstall -g ccmux-cli && npm install -g @suisya-systems/renga`

### バイナリを直接ダウンロード

[Releases](https://github.com/suisya-systems/renga/releases) から取得:

| OS | ファイル |
|----|------|
| Windows (x64) | `renga-windows-x64.exe` |
| macOS (Apple Silicon) | `renga-macos-arm64` |
| macOS (Intel) | `renga-macos-x64` |
| Linux (x64) | `renga-linux-x64` |

> **Windows:** コード署名していないため Microsoft Defender SmartScreen が警告を出すことがあります。「詳細情報」→「実行」で開いてください。未署名 OSS ではよくある挙動です。

> **macOS / Linux:** ダウンロード後に `chmod +x renga-*` で実行権限を付けてください。

### ソースからビルド

```bash
git clone https://github.com/suisya-systems/renga.git
cd renga
cargo build --release
# 出来上がり: target/release/renga (Windows なら renga.exe)
```

[Rust](https://rustup.rs/) のツールチェインが必要です。

PR を送る予定があるなら、クローン後に一度だけ git hooks を有効化しておいてください:

```bash
git config core.hooksPath .githooks
```

pre-commit hook が `cargo fmt --all -- --check` を走らせるので、整形漏れが CI ではなく手元で落ちます。既存の `.git/hooks` を勝手に書き換えないよう opt-in にしています。

## 使い方

```bash
renga
```

好きなディレクトリで起動してください。ファイルツリーにはそのディレクトリが表示されます。

### 起動オプション

- `--min-pane-width <COLS>` — 分割後の各ペインが確保する最小幅 (デフォルト `20`)。これを下回る分割は拒否します。`0` を渡した場合は `1` に丸めて、幅 0 のペインが生まれないようにします。
- `--min-pane-height <ROWS>` — 分割後の最小行数 (デフォルト `5`)。`--min-pane-width` と同じ丸め規則。
- `--ime-freeze-panes[=BOOL]` — IME overlay を開いている間、背後のペインの再描画を止めます (デフォルト `true`)。日本語入力中に Claude の Thinking スピナーや裏で流れる出力がちらついて候補窓を邪魔するのを防ぎます。overlay を閉じた瞬間に最新の画面へ追いつきます。overlay を開かないユーザー (IME を使わない人) には影響しないので ON がデフォルト。入力中も生の再描画を見たい場合は `=false` で無効化できます。`config.toml` の `[ime] freeze_panes_on_overlay` でも指定できます。
- `--ime-overlay-catchup-ms <MS>` — `--ime-freeze-panes` が有効なとき、指定ミリ秒ごとに 1 フレームだけ再描画を挟み込みます (デフォルト `3000` ms — README の sweet spot。ちらつきはほぼ気にならないまま、Claude の出力を追える程度の間隔で更新)。完全凍結したいなら `0` を渡してください。`100` 未満は `100` に丸めます。`config.toml` の `[ime] overlay_catchup_ms` でも指定できます。
- `--lang <auto\|ja\|en>` — ステータスバーのヒントやプレビューのエラーメッセージの表示言語 (デフォルト `auto`)。`auto` は OS ロケールを見て、`ja` 系なら日本語、それ以外は英語にフォールバックします。`ja` / `en` はロケールに関わらず言語を固定します。大小文字は区別しません (`--lang JA` / `--lang En` も通ります)。`config.toml` の `[ui] lang` でも指定できます。

## 設定ファイル

必須ではありません。置く場合は以下のパスに TOML ファイルを作ります。

- **Linux**: `$XDG_CONFIG_HOME/renga/config.toml` (なければ `~/.config/renga/config.toml`)
- **macOS**: `~/Library/Application Support/renga/config.toml`
- **Windows**: `%APPDATA%\renga\config.toml`

ファイルが無い、書式が壊れている場合は stderr に警告を出してデフォルト値で起動します。設定ミスで renga が立ち上がらなくなることはありません。未知のセクションやキーは互換性のため無視します。

### `[ime]` — IME overlay

ホストターミナル側の IME (日本語 / 中国語 / 韓国語など) を扱うための overlay です (Issue #25 / PR #36)。

```toml
[ime]
mode = "hotkey"   # "hotkey" | "off"
```

| 値 | 動作 |
|-------|----------|
| `hotkey` (デフォルト) | `Ctrl+;` で現在のペインに overlay を開きます。`Alt+;` / `Alt+I` は `Ctrl+;` を食ってしまうターミナル (WSL + Windows Terminal、Linux 上の VS Code ターミナル、一部の tmux 設定など) のフォールバックです。 |
| `off` | `Ctrl+;` を何もせず飲み込みます。IME を使わない人や、ターミナル側で IME の位置合わせがうまく動いている人向け。 |

CLI フラグ `--ime hotkey|off` は config を 1 回だけ上書きします。優先順位は **CLI > config > デフォルト**。

> 以前は `Claude` のペインにフォーカスするたびに overlay が自動で開く `always` モードもありましたが、実運用で不安定だったため削除しました。フォーカス直後から overlay を使いたい場合は `Ctrl+;` を 1 回押してください。

### 日本語 (CJK) IME での入力

**ちらつき防止 (freeze) + 3 秒ごとの catch-up は on がデフォルト**です (上の flag 表を参照)。追加の設定不要で、ペインにフォーカスして `Ctrl+;` を押せば overlay が開き、裏のペインは凍結されて 3 秒ごとに 1 フレームだけ更新されます。

overlay を開かないユーザー (IME を使わない人) には影響しないので ON をデフォルトにしています。どうしても入力中に生の再描画を見たい / 完全凍結にしたい場合は `config.toml` で上書きできます:

```toml
[ime]
freeze_panes_on_overlay = false    # 入力中も生の再描画
# あるいは
overlay_catchup_ms = 0             # 完全凍結 (catch-up 無効)
```

あとはペインにフォーカスして `Ctrl+;` を押せば overlay が開き、凍結 + 周期再描画が自動で効きます。

![ターミナル中央に開いた IME overlay。日本語の変換候補窓がキャレット直下に表示され、背後の Claude ペインは凍結している](ime-overlay.png)

**どんな体験になるか:**

1. **overlay は必要なときだけ開く。** Claude のペインで `Ctrl+;` を押すと、画面中央に複数行の入力ボックスが出ます。IME の候補窓が入力ボックス内のキャレットに吸着するので、長い日本語を変換している最中に候補窓が画面を跳ね回ることがなくなります (Issue #25)。
2. **背後のちらつきが止まる。** overlay が開いている間は裏のペインを凍結するため、Claude の Thinking スピナーや流れてくるトークンが再描画を起こしません。入力に集中できます。
3. **それでも進捗は見える。** 3 秒ごとに 1 フレームだけ凍結を解除するので、Claude の出力がどこまで進んでいるかは確認できます。`--ime-overlay-catchup-ms` で間隔を調整してください。完全凍結なら `0`、3 秒でも落ち着かないなら `5000`。
4. **複数行のドラフトをそのまま書ける。** `Enter` は改行、送信は `Alt+Enter` (macOS は `Option+Return`)、Windows Terminal / wezterm / VS Code なら `Ctrl+Enter` でも送れます。詳しいキーマップは次節を参照。
5. **一旦閉じても下書きは残る。** `Esc` / `Ctrl+C` で overlay を閉じれば、ペインの様子を見たり renga のペイン操作キー (`Ctrl+D` 分割、`Ctrl+Left/Right` フォーカス切替など) を使ったりできます。同じペインで開き直せば、直前のドラフトがそのまま戻ります。

### IME overlay のキーバインド

overlay は画面中央に複数行の入力ボックスとして開き、IME の候補窓はその中のキャレットに吸着します。

| キー | 動作 |
|-----|--------|
| `Enter` | 改行 (`Shift+Enter` でも同じ) |
| `Alt+Enter` | 送信して overlay を閉じる (主要ターミナル全般で動作、macOS では `Option+Return` — Option が効かない場合は [macOS: Option をメタキーにする](#macos-option-をメタキーにする) を参照) |
| `Ctrl+Enter` | 送信 (Windows Terminal / wezterm / VS Code / 多くの Linux ターミナル) |
| `Esc` / `Ctrl+C` | overlay を閉じる。同じペインで開き直すとドラフトを復元 |
| `←` `→` `↑` `↓` | カーソル移動 |
| `Home` / `End` | 現在行の先頭 / 末尾 |
| `Ctrl+Home` / `Ctrl+End` | バッファ全体の先頭 / 末尾 |
| `Backspace` | キャレット左の 1 文字を削除 |

### `[ui]` — UI 言語

ステータスバーのヒントやプレビューのエラーメッセージで使う言語を切り替えます。renga は元々日本語話者向けのプロジェクトとして日本語ハードコードでしたが、OS ロケールで自動切替する作りに変更しました。

```toml
[ui]
lang = "auto"   # "auto" | "ja" | "en"
```

| 値 | 動作 |
|-------|----------|
| `auto` (デフォルト) | `sys-locale` クレート経由で OS ロケールを検出 (Unix は `nl_langinfo`、Windows は `GetUserDefaultLocaleName` をラップ)。ロケール名が `ja` で始まれば日本語、それ以外は英語。`LANG` / `LC_*` が未設定でも OS レベルのロケールから判定できるので、素の Windows Terminal + PowerShell でも正しく動きます。 |
| `ja` | ロケールに関わらず日本語を強制。 |
| `en` | ロケールに関わらず英語を強制。 |

CLI フラグ `--lang auto|ja|en` は config を 1 回だけ上書きします。CLI (`--lang JA`) でも TOML (`lang = "Ja"`) でも大小文字を区別しません。優先順位は **CLI > config > OS ロケール検出 > 英語フォールバック**。

## Claude Code と Codex ペイン間のメッセージング

同じ renga タブに並べた Claude Code と Codex のペイン同士が、お互いにメッセージを送り合えるようになります。片方のエージェントに「これを調べておいて」と頼んだり、失敗したテストの原因追いをもう片方に引き継いだりといった連絡を、ユーザーが手でコピペして中継しなくても進められます。Claude は `<channel source="renga-peers">` タグで受け取り、Codex は `check_messages` で queue を引き取ります。

届いたメッセージは受信側 Claude Code のコンテキストに `<channel source="renga-peers">` タグ付きで入るので、**ユーザーが打ち込んだ入力とははっきり区別**されます。

> **[`claude-peers-mcp`](https://github.com/happy-ryo/claude-peers-mcp) との違い** — ツール名や引数はほぼ同じですが、スコープの決め方が違います。`claude-peers-mcp` は `cwd` / `git_root` / `PID` からペアを推測するので、同じディレクトリを複数案件で開いていると混ざりがち。renga-peers は **「ユーザーがこのタブに並べて置いた」という物理配置そのもの**をスコープにするので、推測に頼らず境界がはっきりします。チャンネル名が違う (`server:renga-peers` / `server:claude-peers`) ので、両方同時にインストールしていても衝突しません。

### セットアップ (1 回だけ)

```bash
renga mcp install --client claude
renga mcp install --client codex   # Codex peer も使う場合
```

いま走っている `renga` バイナリを `renga-peers` という名前で、選択した client のユーザー設定に登録します。Claude は `claude mcp add-json`、Codex は `codex mcp add` を内部で呼ぶので、client 側の設定ファイル書式を renga が直接追う必要はありません。

同じ登録を複数回実行しても安全で、既に登録があれば内容を表示して止まります。renga をアップグレードしてバイナリのパスが変わったときだけ `--force` で上書きしてください。外したいときは `renga mcp uninstall --client ...`、今どう登録されているかは `renga mcp status --client ...` で確認できます。

### Claude Code / Codex をメッセージング対応で起動する

配送方式は client ごとに違います。

- **Claude Code** は MCP の experimental channel 機能を使うので、起動時に毎回 `--dangerously-load-development-channels server:renga-peers` が必要です。
- **Codex** は `renga mcp install --client codex` で入れた MCP 登録を使います。これが `RENGA_PEER_CLIENT_KIND=codex` を MCP サブプロセスに注入するので、起動自体は plain `codex` で足ります。受信は `check_messages` による pull です。

Codex の auto-nudge は opt-in です。`codex` を起動する親シェルで `RENGA_CODEX_AUTO_NUDGE=1` を設定してください。`renga mcp install --client codex` を実行してあれば、その env var は `renga-peers` の MCP サブプロセスまで自動で引き継がれます。

Claude の起動フラグを毎回手で打たなくて済むように、renga 側から 2 つの経路を用意しています。

- **`Alt+P`** — フォーカス中のペインに `claude --dangerously-load-development-channels server:renga-peers ` を入力してくれる (末尾にスペース、**Enter は押されない**)。そのまま Enter で起動してもいいし、追加の引数 (例 `/foo`) を続けて書いてから Enter でも OK。シェル (bash / zsh / fish / pwsh) の種類を問わず同じ動作。
- **`renga split --role claude`** / **`renga new-tab --role claude`** — 新しいペインを開いたあと、そのペインで自動的に上のフラグ付き Claude Code が立ち上がる。`--command "..."` を明示したときはそちらが優先されるので、カスタム起動のための逃げ道は残します。

Codex を会話の中から増やしたい場合は、`spawn_codex_pane(direction, …)` を使います。

### 2 ペインでのやり取りの例

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

Claude B の次のターンのコンテキストに `<channel source="renga-peers">src/app.rs の handle_split を読んで...</channel>` タグで届き、Claude B は「ユーザーではなく相方からの依頼」と判別した上で要件を処理 → 同じ `send_message` で返信します。

**提供ツール:**

_ペインメッセージング:_

| ツール | 役割 |
|---|---|
| `list_peers` | 同じ renga タブにいる他のペインを返す。自分自身は除外。 |
| `send_message(to_id, message)` | 同じタブの相方にメッセージを送る。数字の id でも、ペインに付けた名前でも指定可能。別タブ宛は何も配送せず成功として返す (他タブのペインを id 探索で列挙されないようにするため)。 |
| `check_messages` | pull 型 client の受信箱を drain する。Claude では主にフォールバック用途だが、Codex ではこれが主受信経路なので、各ターン開始時と長作業の節目で呼ぶ前提。 |
| `set_summary` | v1 では受け付けるだけで保存しない。renga はペイン名と役割 (role) を代わりに使います。 |

_ペイン操作 (`new_tab` を除き同一タブ内):_

| ツール | 役割 |
|---|---|
| `list_panes` | 同じタブの全ペインを id / 名前 / role / フォーカス状態 / cwd / レイアウト位置込みで一覧する。 |
| `spawn_pane(direction, …)` | 指定ペインを分割して新ペインを生やす。`command`・`name`・`role`・`cwd` はどれもオプション。`command="claude"` (または `claude <args>`) を渡したときは `Alt+P` と同じ peer 対応コマンドに自動書き換えするので、毎回 `--dangerously-load-development-channels` を覚えていなくてもメッセージング付きの Claude が立ち上がる。 |
| `spawn_claude_pane(direction, …)` | Claude 起動専用の高レベル API。`command` の代わりに `permission_mode` / `model` / `args[]` を構造化フィールドで受け取り、peer チャネルを必ず有効にした状態で Claude Code を立ち上げる。orchestrator 側で shell クオートや launch policy を管理せず renga 側に集約できるので、`spawn_pane(command="claude ...")` を合成するより agent harness 向き。`args[]` に予約済みフラグ（`--dangerously-load-development-channels` / `--permission-mode` / `--model`）が入っていると `invalid-params` で拒否する。 |
| `spawn_codex_pane(direction, …)` | Codex 起動専用の高レベル API。`args[]` から最終的な `codex ...` コマンドを renga 側で shell-quote して組み立てる。MCP サブプロセス側の `RENGA_PEER_CLIENT_KIND=codex` 登録は `renga mcp install --client codex` に委ねるため、pane 起動自体は plain `codex` でよい。`spawn_pane(command="codex ...")` を LLM に合成させる代わりに使う。 |
| `close_pane(target)` | ペインを閉じる。最後のタブの最後のペインを閉じようとしたときは `last_pane` で拒否。 |
| `focus_pane(target)` | 同じタブ内でフォーカス移動。ユーザーの手元からフォーカスを奪うことになるので使いどころに注意。 |
| `new_tab(…)` | 新しいタブを 1 枚開いてそこへフォーカスを移す。`spawn_pane` と同じ `cwd` オプションと `claude` 自動アップグレードが効く。 |
| `inspect_pane(target, …)` | 別ペインの可視画面をスナップショットして、プロンプト待ち・警告バナー・モード変化などをそのペイン自身に説明させず検出できる。デフォルトはプレーンテキスト、`format="grid"` で行アドレス付き JSON、`lines=N` で末尾 N 行に絞り込み。 |
| `send_keys(target, …)` | 別ペインの PTY に生のキー入力 (`Enter` / `Esc` / 矢印 / `Ctrl+<letter>` / 任意テキストなど) を送る。`send_message` を解釈できない対話プロンプトや TUI を操作したいとき向け。 |
| `set_pane_identity(target, name?, role?)` | 既存ペインの安定 `name` / `role` を付け直す・クリアする。3 ステート: キー省略 = 現状維持、`null` = クリア、文字列 = 設定。`renga --layout ops` を忘れて起動したセッションで secretary ペインに `id = "secretary"` が付いていない、といった状態からのリカバリに使う。全桁数字の name は数値 id と曖昧化するため拒否、同一タブ内の name 衝突も拒否。CLI では `renga rename [--id \| --name \| --focused] [--to-name \| --clear-name] [--to-role \| --clear-role]`。 |

_イベント監視:_

| ツール | 役割 |
|---|---|
| `poll_events(timeout_ms?, since?, types?)` | `pane_started` / `pane_exited` / `events_dropped` を cursor 付き long-poll で受け取る。オーケストレータが毎ターン pane 一覧を総当たりせずに、ワーカーの起動・終了だけを追いたいとき向け。 |

> この `claude` 自動アップグレードは layout TOML (`renga --layout <name>`) 経由で起動するペインにも適用される。layout toml に `command = "claude"` と書けばペイン起動時に peer 対応コマンドへ書き換えられるので、各エントリで毎回 `--dangerously-load-development-channels server:renga-peers` を書く必要はない。

> **ペインの `cwd`** — `spawn_pane` / `new_tab` / `renga split --cwd` / `renga new-tab --cwd` / layout TOML `cwd = "..."` で新ペインの作業ディレクトリを指定できる。絶対パスはそのまま、相対パスは呼び出し元ペインの cwd（MCP）・シェルの cwd（CLI）・renga プロセスの cwd（layout TOML）を基準に解決される。存在しないパスはレイアウトを変更する前に `cwd_invalid` で失敗するので half-mutated なレイアウトにならない。`command` に `cd <dir> && ...` を書くと `claude` 自動アップグレードが効かなくなるので、`cwd` フィールド側で指定するのが推奨。

### うまく動かないとき

- **`list_peers` が "renga not reachable from this peer client" を返す** — client が renga の外で起動されたか、renga ペインの環境変数を引き継げていません。renga のペイン内から起動し直してください（Claude は `Alt+P` / `renga split --role claude`、Codex は `renga mcp install --client codex` 後の plain `codex` または `spawn_codex_pane`）。
- **相手に送ったメッセージが `<channel>` タグで表示されない** — 起動時のフラグ `--dangerously-load-development-channels server:renga-peers` を付け忘れています。`claude` と打つ代わりに `Alt+P` を使えばフラグ付きのコマンドが挿入されるので事故りにくくなります。
- **Codex に送ったのに反応がない** — Codex は pull 型です。メッセージは `check_messages` が呼ばれるまで queue に残ります。Codex 側の prompt / workflow が turn 開始時と長作業の節目で `check_messages` を呼ぶ前提になっているか確認してください。
- **Codex の auto-nudge が動かない** — 既定では無効です。`renga mcp install --client codex` を実行して `RENGA_CODEX_AUTO_NUDGE` の passthrough を入れた上で、renga のペイン内から `RENGA_CODEX_AUTO_NUDGE=1` を設定した親シェルで `codex` を起動してください。
- **`send_keys` が効いていないように見える** — `send_keys` は target ペインの PTY に生の入力バイトを書き込むだけで、帯域外の「承認」操作ではありません。まず `inspect_pane(target=..., lines=20)` で本当に入力待ちか確認し、レイアウトが動く運用ではフォーカス推測ではなく安定した pane `name` を target に使ってください。
- **`poll_events` が想定より早く `events: []` を返す** — `types=[...]` フィルタは返却結果を絞るだけで、非一致イベントでも long-poll は解除されて `next_since` は前進します。返ってきた cursor でそのまま再 poll してください。`events_dropped` が来た場合だけ、1 回 `list_panes` で再同期すると安全です。
- **renga をアップグレードしたら繋がらなくなった** — 登録されているバイナリのパスが古いです。`renga mcp install --client claude --force` や `renga mcp install --client codex --force` で今の `renga` に更新してください。

## キーバインド

> **macOS ユーザーへ:** macOS のターミナルは既定で `Option+<キー>` を Unicode 入力 (`å` `∫` など) に割り当てているため、そのままだと `Alt+T` / `Alt+P` / `Alt+1..9` / `Alt+Left/Right` が renga まで届きません。1 行の設定で解決できます → [macOS: Option をメタキーにする](#macos-option-をメタキーにする) (WezTerm / iTerm2 / Alacritty / Ghostty / Kitty / Terminal.app 別に記載)。

### ペインモード (通常状態)

| キー | 動作 |
|-----|--------|
| `Ctrl+D` | 縦分割 |
| `Ctrl+E` | 横分割 |
| `Ctrl+W` | ペインを閉じる (最後の 1 つならタブごと閉じる) |
| `Alt+T` / `Ctrl+T` | 新しいタブ |
| `Alt+1..9` | 指定番号のタブに移動 |
| `Alt+Left/Right` | 前 / 次のタブ |
| `Alt+R` | タブ名を変更 (セッション内のみ) |
| `Alt+S` | ステータスバー表示切替 |
| `Alt+P` | フォーカス中のペインにメッセージング対応の Claude Code 起動コマンドを入力 ([詳細](#claude-code-ペイン同士のメッセージング)) |
| `Ctrl+F` | ファイルツリー表示切替 |
| `Ctrl+P` | プレビューとターミナルの位置を入れ替え |
| `Ctrl+Right/Left` | サイドバー / プレビュー / ペイン間のフォーカス移動 |
| `Ctrl+;` / `Alt+;` / `Alt+I` | IME overlay を開く (上記参照)。`Alt+;` / `Alt+I` は `Ctrl+;` を食ってしまうターミナル (WSL + Windows Terminal、Linux 上の VS Code ターミナル、一部の tmux 設定など) のフォールバックです。 |
| `Ctrl+Q` | renga 終了 |

### macOS: Option をメタキーにする

macOS のターミナルは既定で `Option+<キー>` を Unicode 入力 (`å` `∫` `π` など) に割り当てているため、renga の `Alt+T` / `Alt+P` / `Alt+R` / `Alt+S` / `Alt+1..9` / `Alt+Left/Right` が発火しません。Option をメタキー扱いに切り替えれば解決します。どの端末も 1 行の設定変更で済みます。**素の Terminal.app はあまりおすすめしません** (IME 対応・ligature・画像プレビューいずれも下表のモダン端末の方が上なので、乗り換えが結局近道です)。

| ターミナル | 設定 |
|---|---|
| **WezTerm** (`~/.wezterm.lua`) | `config.send_composed_key_when_left_alt_is_pressed = false` <br> `config.send_composed_key_when_right_alt_is_pressed = false` |
| **iTerm2** | Settings → Profiles → Keys → **Left Option key** と **Right Option key** を **Esc+** に設定 |
| **Alacritty** (`~/.config/alacritty/alacritty.toml`) | `[window]` <br> `option_as_alt = "Both"` (`"OnlyLeft"` / `"OnlyRight"` も可) |
| **Ghostty** (`~/.config/ghostty/config`) | `macos-option-as-alt = true` |
| **Kitty** (`~/.config/kitty/kitty.conf`) | `macos_option_as_alt yes` |
| **Terminal.app** | Settings → Profiles → Keyboard → **Use Option as Meta key** にチェック |

**既知の制約**

- 一部の macOS IME (ことえりの「英字」切替、日本語キー配列など) は Option を独自に使っているため、Meta 化と競合する場合があります。IME が壊れたら `OnlyLeft` / `OnlyRight` のように片側だけ Meta にするのが妥協点です。
- `Alt+1..9` は macOS の Mission Control / Spaces のショートカットと衝突することがあります。OS 側に奪われる場合はタブ巡回を `Alt+Left/Right` で代替してください。

### ファイルツリーモード (`Ctrl+F` 押下後)

| キー | 動作 |
|-----|--------|
| `j` / `k` | 上下移動 |
| `Enter` | ファイルを開く / ディレクトリ展開 (インライン) |
| `h` | ツリーの root を 1 つ上のディレクトリに変更 |
| `l` | 選択したディレクトリに root を移動 (ファイルに対しては no-op) |
| `c` | 現ペインを左右分割し、右側の新ペインを選択位置のディレクトリ (ファイル選択時は親、空のときはツリー root) で開いた上で、Claude + peer MCP の起動コマンドをプロンプトに流し込む。`Alt+P` と同様に Enter 未押下なので、内容を確認してから Enter で起動できる。 |
| `v` | `c` と同じ挙動で上下分割 (下側に新ペイン)。 |
| `.` | 隠しファイル表示切替 |
| `Esc` | ペインに戻る |

### プレビューモード (プレビューにフォーカス中)

| キー | 動作 |
|-----|--------|
| `j` / `k` | 縦スクロール |
| `h` / `l` | 横スクロール |
| `Ctrl+W` | プレビューを閉じる |
| `Esc` | ペインに戻る |

### マウス

| 操作 | 動作 |
|--------|--------|
| ペインをクリック | そのペインにフォーカス |
| タブをクリック | タブ切替 |
| タブをダブルクリック | タブ名変更 |
| `+` をクリック | 新しいタブ |
| 境界をドラッグ | パネルのリサイズ |
| ホイールスクロール | ファイルツリー / プレビュー / ターミナル履歴。マウスレポートに opt-in している TUI (Claude Code `/tui fullscreen`, vim, lazygit, less 等) では代わりにアプリ側にホイールイベントが届きます。 |
| ペイン内クリック / ドラッグ | 通常はテキスト選択 (コピー用)。マウスレポートに opt-in している TUI 上ではアプリにクリックが転送されるので、アプリ側のボタンやカーソル移動が動きます。`Shift` を押しながらドラッグすると強制的に renga のテキスト選択になります (tmux / alacritty と同じ escape hatch)。 |

ホイール / クリック両方の転送は `RENGA_DISABLE_MOUSE_FORWARD=1` で無効化できます。renga をネストしている場合や、マウスプロトコルの encoding が合わなくて内側のアプリが混乱する環境向けの逃げ道です。

## 構成

```
src/
├── main.rs       # エントリポイント、イベントループ、panic フック
├── app.rs        # ワークスペース / タブ / レイアウト、キー & マウス処理
├── pane.rs       # PTY 管理、vt100 エミュレーション、シェル検出
├── ui.rs         # ratatui 描画、テーマ、レイアウト
├── filetree.rs   # ファイルツリーのスキャンとナビゲーション
└── preview.rs    # プレビュー (シンタックスハイライト + 画像)
```

**設計上の選択:**

- ターミナルエミュレーションには `vt100` クレートを使用 (ANSI ストリップではない)。Claude Code のインタラクティブ UI のために必要。
- 分割レイアウトは比率を持たせた二分木で再帰的に表現。
- PTY ごとに reader スレッドを立て、mpsc チャネルで main ループに流す。
- `cd` 追従は OSC 7 を使用。
- 描画は dirty フラグ方式で、アイドル時の CPU コストを最小化。

## 技術スタック

- [ratatui](https://ratatui.rs/) + [crossterm](https://github.com/crossterm-rs/crossterm) — TUI フレームワーク
- [portable-pty](https://github.com/nickelc/portable-pty) — PTY 抽象化 (Windows では ConPTY)
- [vt100](https://crates.io/crates/vt100) — ターミナルエミュレーション
- [syntect](https://github.com/trishume/syntect) — シンタックスハイライト

## Claude Code の参考情報

Claude Code が初めてなら [Claude Code Academy](https://claude-code-academy.dev) のチュートリアルが参考になります。

## 経緯と謝辞

renga は 2026 年初頭に [Shin-sibainu/ccmux](https://github.com/Shin-sibainu/ccmux) から派生して始まり、その後は独立して進化してきました。peer メッセージング用の MCP チャネル、Claude 認識付きのペイン枠表示、IME 合成 overlay、layout TOML、日英バイリンガル UX といった機能はすべて renga 独自の実装です。両プロジェクトはもはやバージョンごとに追従する関係ではなく、renga は独自の semver 系列を独自のペースでリリースしています (詳細は [`BRANCHING.md`](./BRANCHING.md) の divergence policy を参照)。

ratatui ベースのペインツリー、vt100 を使ったターミナルエミュレーション、クロスプラットフォーム PTY レイヤーといった出発点を提供してくれた [Shin-sibainu](https://github.com/Shin-sibainu) 氏と上流 ccmux の作者陣に感謝します。上流由来のコミット履歴はリポジトリの git log にそのまま保存されており、`Shin-sibainu` の MIT 著作権表示はライセンス義務として [`LICENSE`](./LICENSE) に保持しています。

## ライセンス

MIT — [`LICENSE`](./LICENSE) を参照。上流 `Shin-sibainu/ccmux` の著作権表示はライセンス条項に従って保持されており、renga の追加実装も同じ MIT 条件で公開しています。
