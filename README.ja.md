# ccmux (fork)

*Language: [English](./README.md) / 日本語*

Claude Code のセッションを分割ペインで同時に動かすための、小さな TUI マルチプレクサ。

[Claude Code](https://docs.anthropic.com/en/docs/claude-code) を複数起動して横に並べて使いたい、という用途に絞って作られています。

> **これは [Shin-sibainu/ccmux](https://github.com/Shin-sibainu/ccmux) のフォークです。** 上流を定期的に取り込みつつ、独自機能を先行して足しています。npm では別パッケージ `ccmux-fork` として配布しています。ブランチの使い分けは [`BRANCHING.md`](./BRANCHING.md) を参照してください。

![ccmux スクリーンショット](screenshot.png)

## できること

- **ペイン分割** — 縦横に分割、各ペインは独立したシェル (PTY)
- **タブ** — プロジェクトごとに独立したワークスペースをタブで切替
- **Claude Code ペイン同士のメッセージング** — 同じタブに並べた Claude Code 同士が会話できる。届いたメッセージはユーザー入力と区別される `<channel source="ccmux-peers">` タグ付きでコンテキストに入る ([詳細](#claude-code-ペイン同士のメッセージング))
- **ファイルツリー** — アイコン付きのサイドバー、ディレクトリは展開/折りたたみ
- **プレビュー** — シンタックスハイライト付き、画像ファイルも表示 (Sixel / Kitty / iTerm2 / halfblocks 自動選択)
- **Claude Code の自動検出** — Claude Code が動いているペインは枠がオレンジになる
- **`cd` に追従** — ディレクトリを移動するとファイルツリーとタブ名も自動で切り替わる
- **マウス操作** — クリックでフォーカス、境界ドラッグでリサイズ、ホイールで履歴スクロール
- **10,000 行のスクロールバック** (ペインごと)
- **ダークテーマ** (Claude 風のカラースキーム)
- **Windows / macOS / Linux** 対応、単一バイナリ (約 1 MB、追加ランタイム不要)

## インストール

### npm (おすすめ)

```bash
npm install -g ccmux-fork
```

> 上流の `ccmux-cli` を入れている場合は: `npm uninstall -g ccmux-cli && npm install -g ccmux-fork`

### バイナリを直接ダウンロード

[Releases](https://github.com/happy-ryo/ccmux/releases) から取得:

| OS | ファイル |
|----|------|
| Windows (x64) | `ccmux-windows-x64.exe` |
| macOS (Apple Silicon) | `ccmux-macos-arm64` |
| macOS (Intel) | `ccmux-macos-x64` |
| Linux (x64) | `ccmux-linux-x64` |

> **Windows:** コード署名していないため Microsoft Defender SmartScreen が警告を出すことがあります。「詳細情報」→「実行」で開いてください。未署名 OSS ではよくある挙動です。

> **macOS / Linux:** ダウンロード後に `chmod +x ccmux-*` で実行権限を付けてください。

### ソースからビルド

```bash
git clone https://github.com/happy-ryo/ccmux.git
cd ccmux
cargo build --release
# 出来上がり: target/release/ccmux (Windows なら ccmux.exe)
```

[Rust](https://rustup.rs/) のツールチェインが必要です。

PR を送る予定があるなら、クローン後に一度だけ git hooks を有効化しておいてください:

```bash
git config core.hooksPath .githooks
```

pre-commit hook が `cargo fmt --all -- --check` を走らせるので、整形漏れが CI ではなく手元で落ちます。既存の `.git/hooks` を勝手に書き換えないよう opt-in にしています。

## 使い方

```bash
ccmux
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

- **Linux**: `$XDG_CONFIG_HOME/ccmux/config.toml` (なければ `~/.config/ccmux/config.toml`)
- **macOS**: `~/Library/Application Support/ccmux/config.toml`
- **Windows**: `%APPDATA%\ccmux\config.toml`

ファイルが無い、書式が壊れている場合は stderr に警告を出してデフォルト値で起動します。設定ミスで ccmux が立ち上がらなくなることはありません。未知のセクションやキーは互換性のため無視します。

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
5. **いつでも抜けられる。** `Esc` で overlay を閉じれば、ccmux のペイン操作キー (`Ctrl+D` 分割、`Ctrl+Left/Right` フォーカス切替など) がすぐ使えるようになります。

### IME overlay のキーバインド

overlay は画面中央に複数行の入力ボックスとして開き、IME の候補窓はその中のキャレットに吸着します。

| キー | 動作 |
|-----|--------|
| `Enter` | 改行 (`Shift+Enter` でも同じ) |
| `Alt+Enter` | 送信して overlay を閉じる (主要ターミナル全般で動作、macOS では `Option+Return` — Option が効かない場合は [macOS: Option をメタキーにする](#macos-option-をメタキーにする) を参照) |
| `Ctrl+Enter` | 送信 (Windows Terminal / wezterm / VS Code / 多くの Linux ターミナル) |
| `Esc` / `Ctrl+C` | キャンセル — overlay を閉じてバッファを破棄 |
| `←` `→` `↑` `↓` | カーソル移動 |
| `Home` / `End` | 現在行の先頭 / 末尾 |
| `Ctrl+Home` / `Ctrl+End` | バッファ全体の先頭 / 末尾 |
| `Backspace` | キャレット左の 1 文字を削除 |

### `[ui]` — UI 言語

ステータスバーのヒントやプレビューのエラーメッセージで使う言語を切り替えます。ccmux は元々日本語話者向けのフォークなので日本語ハードコードでしたが、OS ロケールで自動切替する作りに変更しました。

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

## Claude Code ペイン同士のメッセージング

同じ ccmux タブに並べた Claude Code のペイン同士が、お互いにメッセージを送り合えるようになります。片方の Claude Code に「これを調べておいて」と頼んだり、失敗したテストの原因追いをもう片方に引き継いだりといった連絡を、ユーザーが手でコピペして中継しなくても進められます。

届いたメッセージは受信側 Claude Code のコンテキストに `<channel source="ccmux-peers">` タグ付きで入るので、**ユーザーが打ち込んだ入力とははっきり区別**されます。

> **[`claude-peers-mcp`](https://github.com/happy-ryo/claude-peers-mcp) との違い** — ツール名や引数はほぼ同じですが、スコープの決め方が違います。`claude-peers-mcp` は `cwd` / `git_root` / `PID` からペアを推測するので、同じディレクトリを複数案件で開いていると混ざりがち。ccmux-peers は **「ユーザーがこのタブに並べて置いた」という物理配置そのもの**をスコープにするので、推測に頼らず境界がはっきりします。チャンネル名が違う (`server:ccmux-peers` / `server:claude-peers`) ので、両方同時にインストールしていても衝突しません。

### セットアップ (1 回だけ)

```bash
ccmux mcp install
```

いま走っている `ccmux` バイナリを `ccmux-peers` という名前で Claude Code のユーザー設定に登録します。内部で `claude mcp add-json` を呼んでいるだけなので、Claude Code 側の設定ファイルの書式が将来変わっても ccmux は追従する必要がありません。

同じ登録を複数回実行しても安全で、既に登録があれば内容を表示して止まります。ccmux をアップグレードしてバイナリのパスが変わったときだけ `--force` で上書きしてください。外したいときは `ccmux mcp uninstall`、今どう登録されているかは `ccmux mcp status` で確認できます。

### Claude Code をメッセージング対応で起動する

メッセージングは MCP の experimental channel 機能を使っているので、Claude Code 起動時に毎回 `--dangerously-load-development-channels server:ccmux-peers` というフラグが必要です。現状これを省略する環境変数などは Claude Code 側にないので、毎回手で打たなくて済むように ccmux 側から 2 つの経路を用意しています。

- **`Alt+P`** — フォーカス中のペインに `claude --dangerously-load-development-channels server:ccmux-peers ` を入力してくれる (末尾にスペース、**Enter は押されない**)。そのまま Enter で起動してもいいし、追加の引数 (例 `/foo`) を続けて書いてから Enter でも OK。シェル (bash / zsh / fish / pwsh) の種類を問わず同じ動作。
- **`ccmux split --role claude`** / **`ccmux new-tab --role claude`** — 新しいペインを開いたあと、そのペインで自動的に上のフラグ付き Claude Code が立ち上がる。`--command "..."` を明示したときはそちらが優先されるので、カスタム起動のための逃げ道は残します。

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

Claude B の次のターンのコンテキストに `<channel source="ccmux-peers">src/app.rs の handle_split を読んで...</channel>` タグで届き、Claude B は「ユーザーではなく相方からの依頼」と判別した上で要件を処理 → 同じ `send_message` で返信します。

**提供ツール:**

_ペインメッセージング:_

| ツール | 役割 |
|---|---|
| `list_peers` | 同じ ccmux タブにいる他のペインを返す。自分自身は除外。 |
| `send_message(to_id, message)` | 同じタブの相方にメッセージを送る。数字の id でも、ペインに付けた名前でも指定可能。別タブ宛は何も配送せず成功として返す (他タブのペインを id 探索で列挙されないようにするため)。 |
| `check_messages` | 受信箱を手動で取り出す。通常は channel 経由で push されてくるので出番は少なく、取りこぼしを疑うときの確認用。 |
| `set_summary` | v1 では受け付けるだけで保存しない。ccmux はペイン名と役割 (role) を代わりに使います。 |

_ペイン操作 (`new_tab` を除き同一タブ内):_

| ツール | 役割 |
|---|---|
| `list_panes` | 同じタブの全ペインを id / 名前 / role / フォーカス状態 / レイアウト位置込みで一覧する。 |
| `spawn_pane(direction, …)` | 指定ペインを分割して新ペインを生やす。`command`・`name`・`role` はどれもオプション。`command="claude"` (または `claude <args>`) を渡したときは `Alt+P` と同じ peer 対応コマンドに自動書き換えするので、毎回 `--dangerously-load-development-channels` を覚えていなくてもメッセージング付きの Claude が立ち上がる。 |
| `close_pane(target)` | ペインを閉じる。最後のタブの最後のペインを閉じようとしたときは `last_pane` で拒否。 |
| `focus_pane(target)` | 同じタブ内でフォーカス移動。ユーザーの手元からフォーカスを奪うことになるので使いどころに注意。 |
| `new_tab(…)` | 新しいタブを 1 枚開いてそこへフォーカスを移す。`spawn_pane` と同じ `claude` 自動アップグレードが効く。 |

### うまく動かないとき

- **`list_peers` が "ccmux not reachable from this Claude Code instance" を返す** — Claude Code が ccmux の外で起動されたか、ccmux ペインの環境変数を引き継げていません。ccmux のペイン内で `Alt+P` か `ccmux split --role claude` から起動し直してください。
- **相手に送ったメッセージが `<channel>` タグで表示されない** — 起動時のフラグ `--dangerously-load-development-channels server:ccmux-peers` を付け忘れています。`claude` と打つ代わりに `Alt+P` を使えばフラグ付きのコマンドが挿入されるので事故りにくくなります。
- **ccmux をアップグレードしたら繋がらなくなった** — 登録されているバイナリのパスが古いです。`ccmux mcp install --force` で今の `ccmux` に更新してください。

## キーバインド

> **macOS ユーザーへ:** macOS のターミナルは既定で `Option+<キー>` を Unicode 入力 (`å` `∫` など) に割り当てているため、そのままだと `Alt+T` / `Alt+P` / `Alt+1..9` / `Alt+Left/Right` が ccmux まで届きません。1 行の設定で解決できます → [macOS: Option をメタキーにする](#macos-option-をメタキーにする) (WezTerm / iTerm2 / Alacritty / Ghostty / Kitty / Terminal.app 別に記載)。

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
| `Ctrl+Q` | ccmux 終了 |

### macOS: Option をメタキーにする

macOS のターミナルは既定で `Option+<キー>` を Unicode 入力 (`å` `∫` `π` など) に割り当てているため、ccmux の `Alt+T` / `Alt+P` / `Alt+R` / `Alt+S` / `Alt+1..9` / `Alt+Left/Right` が発火しません。Option をメタキー扱いに切り替えれば解決します。どの端末も 1 行の設定変更で済みます。**素の Terminal.app はあまりおすすめしません** (IME 対応・ligature・画像プレビューいずれも下表のモダン端末の方が上なので、乗り換えが結局近道です)。

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
| ペイン内クリック / ドラッグ | 通常はテキスト選択 (コピー用)。マウスレポートに opt-in している TUI 上ではアプリにクリックが転送されるので、アプリ側のボタンやカーソル移動が動きます。`Shift` を押しながらドラッグすると強制的に ccmux のテキスト選択になります (tmux / alacritty と同じ escape hatch)。 |

ホイール / クリック両方の転送は `CCMUX_DISABLE_MOUSE_FORWARD=1` で無効化できます。ccmux をネストしている場合や、マウスプロトコルの encoding が合わなくて内側のアプリが混乱する環境向けの逃げ道です。

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

## ライセンス

MIT
