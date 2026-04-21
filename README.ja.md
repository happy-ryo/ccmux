# ccmux (fork)

*Language: [English](./README.md) / 日本語*

Claude Code のセッションを分割ペインで同時に動かすための、小さな TUI マルチプレクサ。

[Claude Code](https://docs.anthropic.com/en/docs/claude-code) を複数起動して横に並べて使いたい、という用途に絞って作られています。

> **これは [Shin-sibainu/ccmux](https://github.com/Shin-sibainu/ccmux) のフォークです。** 上流を定期的に取り込みつつ、独自機能を先行して足しています。npm では別パッケージ `ccmux-fork` として配布しています。ブランチの使い分けは [`BRANCHING.md`](./BRANCHING.md) を参照してください。

![ccmux スクリーンショット](screenshot.png)

## できること

- **ペイン分割** — 縦横に分割、各ペインは独立したシェル (PTY)
- **タブ** — プロジェクトごとに独立したワークスペースをタブで切替
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

## 使い方

```bash
ccmux
```

好きなディレクトリで起動してください。ファイルツリーにはそのディレクトリが表示されます。

### 起動オプション

- `--min-pane-width <COLS>` — 分割後の各ペインが確保する最小幅 (デフォルト `20`)。これを下回る分割は拒否します。`0` を渡した場合は `1` に丸めて、幅 0 のペインが生まれないようにします。
- `--min-pane-height <ROWS>` — 分割後の最小行数 (デフォルト `5`)。`--min-pane-width` と同じ丸め規則。
- `--ime-freeze-panes[=BOOL]` — IME overlay を開いている間、背後のペインの再描画を止めます (デフォルト `false`)。日本語入力中に Claude の Thinking スピナーや裏で流れる出力がちらついて候補窓を邪魔するのを防ぎます。overlay を閉じた瞬間に最新の画面へ追いつきます。フラグだけ渡すと有効、`=false` を付けると config 側の `true` を打ち消せます。`config.toml` の `[ime] freeze_panes_on_overlay` でも指定できます。
- `--ime-overlay-catchup-ms <MS>` — `--ime-freeze-panes` が有効なとき、指定ミリ秒ごとに 1 フレームだけ再描画を挟み込みます (デフォルト `0` = 無効、完全に凍結)。overlay を開いたまま Claude の出力が進む様子を確認できます。体感では `3000`〜`5000` がちょうど良く、ちらつきはほぼ気にならないまま、Claude の出力を追える程度の間隔で更新されます。`100` 未満は `100` に丸めます。`config.toml` の `[ime] overlay_catchup_ms` でも指定できます。

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

### 日本語 (CJK) で使うときのおすすめ設定

日本語などの IME 依存が強い言語で Claude にプロンプトを書くことが多いなら、次の 2 つを合わせて起動するのが楽です。

```bash
ccmux --ime-freeze-panes --ime-overlay-catchup-ms 3000
```

毎回同じ設定で起動するなら `config.toml` に書き込んでおきます。

```toml
[ime]
freeze_panes_on_overlay = true
overlay_catchup_ms = 3000
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
| `Alt+Enter` | 送信して overlay を閉じる (主要ターミナル全般で動作、macOS では `Option+Return`) |
| `Ctrl+Enter` | 送信 (Windows Terminal / wezterm / VS Code / 多くの Linux ターミナル) |
| `Esc` / `Ctrl+C` | キャンセル — overlay を閉じてバッファを破棄 |
| `←` `→` `↑` `↓` | カーソル移動 |
| `Home` / `End` | 現在行の先頭 / 末尾 |
| `Ctrl+Home` / `Ctrl+End` | バッファ全体の先頭 / 末尾 |
| `Backspace` | キャレット左の 1 文字を削除 |

## キーバインド

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
| `Ctrl+F` | ファイルツリー表示切替 |
| `Ctrl+P` | プレビューとターミナルの位置を入れ替え |
| `Ctrl+Right/Left` | サイドバー / プレビュー / ペイン間のフォーカス移動 |
| `Ctrl+;` / `Alt+;` / `Alt+I` | IME overlay を開く (上記参照)。`Alt+;` / `Alt+I` は `Ctrl+;` を食ってしまうターミナル (WSL + Windows Terminal、Linux 上の VS Code ターミナル、一部の tmux 設定など) のフォールバックです。 |
| `Ctrl+Q` | ccmux 終了 |

### ファイルツリーモード (`Ctrl+F` 押下後)

| キー | 動作 |
|-----|--------|
| `j` / `k` | 上下移動 |
| `Enter` | ファイルを開く / ディレクトリ展開 (インライン) |
| `h` | ツリーの root を 1 つ上のディレクトリに変更 |
| `l` | 選択したディレクトリに root を移動 (ファイルに対しては no-op) |
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
| ホイールスクロール | ファイルツリー / プレビュー / ターミナル履歴 |

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
