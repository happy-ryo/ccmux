# ccmux (fork)

*他の言語で読む: [English](./README.md)*

Claude Code Multiplexer — 複数の Claude Code インスタンスを TUI 分割ペインで管理する。

複数の [Claude Code](https://docs.anthropic.com/en/docs/claude-code) セッションを並列で動かすことに特化した、軽量なターミナルマルチプレクサです。

> **これは [Shin-sibainu/ccmux](https://github.com/Shin-sibainu/ccmux) のフォーク** で、上流を定期的に取り込みつつ独自機能を先行開発しています。npm では別パッケージ `ccmux-fork` として配布。ブランチ運用方針は [`BRANCHING.md`](./BRANCHING.md) を参照してください。

![ccmux スクリーンショット](screenshot.png)

## 特徴

- **マルチペインターミナル** — 縦 / 横分割、各ペインは独立した PTY シェル
- **タブワークスペース** — 複数のプロジェクトをタブで切替 (クリック対応)
- **ファイルツリーサイドバー** — プロジェクトファイルをアイコン付きで閲覧、ディレクトリ展開 / 折畳
- **シンタックスハイライト付きプレビュー** — 言語を認識したカラーリングでファイル内容を表示
- **Claude Code 検出** — Claude Code 実行中はペイン枠がオレンジに変化
- **cd 追従** — ディレクトリ移動時にファイルツリーとタブ名が自動更新
- **マウス操作** — クリックでフォーカス、境界ドラッグでリサイズ、ホイールで履歴スクロール
- **スクロールバック** — ペインごとに 10,000 行の履歴を保持
- **ダークテーマ** — Claude インスパイアのカラースキーム
- **クロスプラットフォーム** — Windows / macOS / Linux
- **単一バイナリ** — 約 1MB、追加ランタイム不要

## インストール

### npm 経由 (推奨)

```bash
npm install -g ccmux-fork
```

> 上流の `ccmux-cli` を既にインストール済みの場合は: `npm uninstall -g ccmux-cli && npm install -g ccmux-fork`

### バイナリダウンロード

最新版を [Releases](https://github.com/happy-ryo/ccmux/releases) から取得:

| プラットフォーム | ファイル |
|----------|------|
| Windows (x64) | `ccmux-windows-x64.exe` |
| macOS (Apple Silicon) | `ccmux-macos-arm64` |
| macOS (Intel) | `ccmux-macos-x64` |
| Linux (x64) | `ccmux-linux-x64` |

> **Windows:** バイナリはコード署名されていないため Microsoft Defender SmartScreen が警告を出すことがあります。「詳細情報」→「実行」で進めてください。署名されていない OSS では通常の挙動です。

> **macOS / Linux:** ダウンロード後に実行権限を付与してください: `chmod +x ccmux-*`

### ソースからビルド

```bash
git clone https://github.com/happy-ryo/ccmux.git
cd ccmux
cargo build --release
# バイナリは target/release/ccmux (Windows の場合は ccmux.exe)
```

[Rust](https://rustup.rs/) ツールチェーンが必要です。

## 使い方

```bash
ccmux
```

任意のディレクトリから起動します。ファイルツリーはカレントワーキングディレクトリを表示します。

### フラグ

- `--min-pane-width <COLS>` — 分割時に各子ペインが保つ最小列数 (デフォルト `20`)。これより狭くなる分割は拒否。`0` は `1` にクランプして幅 0 の子ペインを防止。
- `--min-pane-height <ROWS>` — 分割時に各子ペインが保つ最小行数 (デフォルト `5`)。`--min-pane-width` と同じクランプルール。
- `--ime-freeze-panes[=BOOL]` — IME composition overlay を開いている間、pane の repaint を凍結 (デフォルト `false`)。日本語入力中に Claude の Thinking スピナーや背景の PTY 出力が引き起こすちらつきを抑制。overlay を閉じると瞬時に最新状態に追いつきます。裸で渡すと有効化、`=false` で config の `true` を強制無効化。`config.toml` の `[ime] freeze_panes_on_overlay` でも設定可。
- `--ime-overlay-catchup-ms <MS>` — `--ime-freeze-panes` が有効なとき、`<MS>` ミリ秒ごとに 1 フレームだけ repaint を再投入し、overlay を開いたまま本文の進捗を確認できるようにする (デフォルト `0` = 無効、純粋な凍結)。`3000`〜`5000` が体感の最適解: ちらつきがほぼ気にならないまま Claude の streaming 出力が読める速度で進む。非ゼロ値は `100` 未満に clamp。`config.toml` の `[ime] overlay_catchup_ms` でも設定可。

## 設定ファイル

任意。以下のパスに TOML ファイルを配置:

- **Linux**: `$XDG_CONFIG_HOME/ccmux/config.toml` (デフォルト `~/.config/ccmux/config.toml`)
- **macOS**: `~/Library/Application Support/ccmux/config.toml`
- **Windows**: `%APPDATA%\ccmux\config.toml`

ファイルが存在しない、または内容が壊れている場合は警告を stderr に出してデフォルトで起動します — 設定の問題で ccmux が起動に失敗することはありません。未知のセクションやキーは前方互換のため無視されます。

### `[ime]` — IME composition overlay

ホストターミナルの IME 入力を扱う overlay の設定 (Issue #25 / PR #36)。

```toml
[ime]
mode = "hotkey"   # "hotkey" | "off"
```

| 値 | 挙動 |
|-------|----------|
| `hotkey` (デフォルト) | `Ctrl+;` でフォーカスされたペインに overlay を開く。 |
| `off` | `Ctrl+;` を静かに無視 — overlay は開かず、キーストロークもシェルに漏らさない。IME を使わないユーザーや、ターミナル側で既に IME 配置を適切に処理している環境向け。 |

CLI フラグ `--ime hotkey|off` は単発実行で config ファイルを上書き。優先順位は **CLI > config ファイル > デフォルト**。

> かつて 3 つ目のモード `always` (Claude pane にフォーカスが乗るたびに overlay を自動起動) が存在しましたが、自動起動が実用水準で安定しなかったため削除されました。フォーカス直後に overlay を使いたい場合は `Ctrl+;` を 1 回押してください。

### JP / CJK IME ユーザー向け推奨セットアップ

日本語 (その他 IME が重要な言語) で Claude にプロンプトを書くことが多いなら、以下の 2 点セットで起動するのがおすすめ:

```bash
ccmux --ime-freeze-panes --ime-overlay-catchup-ms 3000
```

毎回同じ設定で起動したいなら `config.toml` に書き込む:

```toml
[ime]
freeze_panes_on_overlay = true
overlay_catchup_ms = 3000
```

pane にフォーカスを合わせてから `Ctrl+;` を押すと overlay が開き、凍結 + 周期 catch-up が自動で効きます。

![中央 overlay に日本語変換候補窓がキャレット直下に吸着している様子。背後の Claude pane は凍結](ime-overlay.png)

**得られるもの:**

1. **Overlay は明示的に開く。** Claude pane にフォーカスを合わせて `Ctrl+;` を押すと、中央に多行の composition box が表示されます。ホストターミナルの IME 候補窓が box 内のキャレットに吸着するので、長い日本語のフレーズが変換中に画面を飛び回るのが止まります (Issue #25)。
2. **pane のちらつきが止まる。** overlay が開いている間、ccmux は背後の pane を凍結します — Claude の Thinking スピナーや streaming トークンが repaint を発生させなくなるので、IME 候補を邪魔しません。入力に集中できます。
3. **進捗は見える。** 3 秒ごとに ccmux が 1 フレームだけ解凍するので、Claude の出力が進んでいく様子は確認できます。`--ime-overlay-catchup-ms` で間隔を調整: `0` で純粋な凍結、3 秒でも忙しく感じるなら `5000` に。
4. **多行ドラフトが一級市民。** `Enter` は改行挿入。送信は `Alt+Enter` (macOS では `Option+Return`)、または Windows Terminal / wezterm / VS Code では `Ctrl+Enter`。全キーマップは次の項を参照。
5. **緊急脱出。** `Esc` で overlay が閉じるので ccmux の pane 操作ショートカット (`Ctrl+D` 分割、`Ctrl+Left/Right` フォーカス切替など) を使えます。

### IME overlay キーバインド

overlay は画面中央に多行の composition box として開き、ホストターミナルの IME 候補窓はその中のキャレットに吸着します。

| キー | 動作 |
|-----|--------|
| `Enter` | 改行挿入 (`Shift+Enter` も同じ) |
| `Alt+Enter` | バッファを pane に送信して閉じる (全 tier-1 ターミナル対応、macOS の `Option+Return` 含む) |
| `Ctrl+Enter` | 送信の代替キー — Windows Terminal / wezterm / VS Code / 多くの Linux ターミナル |
| `Esc` / `Ctrl+C` | キャンセル — overlay を閉じてバッファを破棄 |
| `←` `→` `↑` `↓` | カーソル移動 |
| `Home` / `End` | 現行行の先頭 / 末尾 |
| `Ctrl+Home` / `Ctrl+End` | バッファ全体の先頭 / 末尾 |
| `Backspace` | キャレット左側の 1 文字を削除 |

## キーバインド

### ペインモード (デフォルト)

| キー | 動作 |
|-----|--------|
| `Ctrl+D` | 縦分割 |
| `Ctrl+E` | 横分割 |
| `Ctrl+W` | ペイン / タブを閉じる |
| `Alt+T` / `Ctrl+T` | 新しいタブ |
| `Alt+1..9` | 指定番号のタブへ |
| `Alt+Left/Right` | 前 / 次のタブ |
| `Alt+R` | タブ名を変更 (セッション限り) |
| `Alt+S` | ステータスバー表示切替 |
| `Ctrl+F` | ファイルツリー表示切替 |
| `Ctrl+P` | プレビュー / ターミナルの配置入替 |
| `Ctrl+Right/Left` | フォーカス巡回 (サイドバー / プレビュー / ペイン) |
| `Ctrl+;` | IME composition overlay を開く (中央多行 — 上記参照) |
| `Ctrl+Q` | 終了 |

### ファイルツリーモード (`Ctrl+F` の後)

| キー | 動作 |
|-----|--------|
| `j` / `k` | 選択移動 |
| `Enter` | ファイルを開く / ディレクトリを展開 |
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

| 操作 | 効果 |
|--------|--------|
| ペインをクリック | ペインをフォーカス |
| タブをクリック | タブ切替 |
| タブをダブルクリック | タブ名変更 |
| `+` をクリック | 新しいタブ |
| 境界をドラッグ | パネルをリサイズ |
| ホイールスクロール | ファイルツリー / プレビュー / ターミナル履歴をスクロール |

## アーキテクチャ

```
src/
├── main.rs       # エントリーポイント、イベントループ、panic フック
├── app.rs        # ワークスペース / タブ状態、レイアウトツリー、キー / マウス処理
├── pane.rs       # PTY 管理、vt100 エミュレーション、シェル検出
├── ui.rs         # ratatui 描画、テーマ、レイアウト
├── filetree.rs   # ファイルツリースキャン、ナビゲーション
└── preview.rs    # シンタックスハイライト付きファイルプレビュー
```

**主要な設計判断:**
- `vt100` クレートでターミナルエミュレーション (ANSI ストリップではない) — Claude Code のインタラクティブ UI のために必要
- 可変比率の再帰分割をバイナリツリーレイアウトで実現
- PTY ごとの reader スレッドから mpsc チャネルで main event loop へ
- OSC 7 検出で cd 自動追従
- idle 時 CPU 使用を最小化する dirty フラグ描画

## 技術スタック

- [ratatui](https://ratatui.rs/) + [crossterm](https://github.com/crossterm-rs/crossterm) — TUI フレームワーク
- [portable-pty](https://github.com/nickelc/portable-pty) — PTY 抽象化 (Windows では ConPTY)
- [vt100](https://crates.io/crates/vt100) — ターミナルエミュレーション
- [syntect](https://github.com/trishume/syntect) — シンタックスハイライト

## Claude Code を学ぶ

Claude Code が初めてなら、チュートリアルとガイドは [Claude Code Academy](https://claude-code-academy.dev) を参照してください。

## ライセンス

MIT
