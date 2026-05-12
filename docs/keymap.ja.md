# キーマップ

*Language: [English](./keymap.md) / 日本語*

renga のフルキーバインド一覧です。README には「最初に覚える 5〜8 個のチートシート」だけを残し、それ以外はすべてこのページにあります。

> **macOS ユーザーへ:** macOS のターミナルは既定で `Option+<キー>` を Unicode 入力 (`å` `∫` など) に割り当てているため、そのままだと `Alt+T` / `Alt+P` / `Alt+1..9` / `Alt+Left/Right` が renga まで届きません。1 行の設定で解決できます → [macOS: Option をメタキーにする](#macos-option-をメタキーにする) (WezTerm / iTerm2 / Alacritty / Ghostty / Kitty / Terminal.app 別に記載)。

## ペインモード (通常状態)

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
| `Alt+P` | フォーカス中のペインにメッセージング対応の Claude Code 起動コマンドを入力 ([`peer-messaging.ja.md`](./peer-messaging.ja.md) 参照) |
| `Ctrl+F` | ファイルツリー表示切替 |
| `Ctrl+P` | プレビューとターミナルの位置を入れ替え |
| `Ctrl+Right/Left` | サイドバー / プレビュー / ペイン間のフォーカス移動 |
| `Ctrl+;` / `Alt+;` / `Alt+I` | IME 合成 overlay を開く (中央に複数行入力ボックス — [`ime.ja.md`](./ime.ja.md) 参照)。`Alt+;` / `Alt+I` は `Ctrl+;` を奪うターミナル (WSL + Windows Terminal、Linux 上の VS Code ターミナル、一部の tmux 設定など) のフォールバック。 |
| `Ctrl+Q` | renga 終了 |

## macOS: Option をメタキーにする

macOS のターミナルは既定で `Option+<キー>` を Unicode 入力 (`å` `∫` `π` など) に割り当てているため、renga の `Alt+T` / `Alt+P` / `Alt+R` / `Alt+S` / `Alt+1..9` / `Alt+Left/Right` が発火しません。Option をメタキー扱いに切り替えれば解決します。どの端末も 1 行の設定変更で済みます。**素の Terminal.app はあまりおすすめしません** — IME 対応・ligature・画像プレビューのどれも下表のモダン端末の方が良いため、乗り換えが結局近道です。

| ターミナル | 設定 |
|---|---|
| **WezTerm** (`~/.wezterm.lua`) | `config.send_composed_key_when_left_alt_is_pressed = false` <br> `config.send_composed_key_when_right_alt_is_pressed = false` |
| **iTerm2** | Settings → Profiles → Keys → **Left Option key** と **Right Option key** を **Esc+** に設定 |
| **Alacritty** (`~/.config/alacritty/alacritty.toml`) | `[window]` <br> `option_as_alt = "Both"` (`"OnlyLeft"` / `"OnlyRight"` も可) |
| **Ghostty** (`~/.config/ghostty/config`) | `macos-option-as-alt = true` |
| **Kitty** (`~/.config/kitty/kitty.conf`) | `macos_option_as_alt yes` |
| **Terminal.app** | Settings → Profiles → Keyboard → **Use Option as Meta key** にチェック |

**既知の制約**

- 一部の macOS IME (ことえりの「英字」切替、日本語キー配列など) は Option を独自に使っているため、Meta 化と競合する場合があります。IME が壊れたら `OnlyLeft` / `OnlyRight` で片側だけ Meta にするのが妥協点です。
- `Alt+1..9` は macOS の Mission Control / Spaces のショートカットと衝突することがあります。OS 側に奪われる場合はタブ巡回を `Alt+Left/Right` で代替してください。

## ファイルツリーモード (`Ctrl+F` 押下後)

| キー | 動作 |
|-----|--------|
| `j` / `k` | 上下移動 |
| `Enter` | ファイルを開く / ディレクトリ展開 (インライン) |
| `h` | ツリーの root を 1 つ上のディレクトリに変更 |
| `l` | 選択したディレクトリに root を移動 (ファイルに対しては no-op) |
| `c` | 現ペインを左右分割し、右側の新ペインを選択位置のディレクトリ (ファイル選択時は親、空のときはツリー root) で開いた上で、Claude + peer MCP の起動コマンドをプロンプトに流し込む。`Alt+P` と同様に Enter 未押下なので、内容を確認してから Enter で起動できる。 |
| `v` | `c` と同じ挙動で上下分割 (下側に新ペイン) |
| `.` | 隠しファイル表示切替 |
| `Esc` | ペインに戻る |

## プレビューモード (プレビューにフォーカス中)

| キー | 動作 |
|-----|--------|
| `j` / `k` | 縦スクロール |
| `h` / `l` | 横スクロール |
| `Ctrl+W` | プレビューを閉じる |
| `Esc` | ペインに戻る |

## マウス

| 操作 | 動作 |
|--------|--------|
| ペインをクリック | そのペインにフォーカス |
| タブをクリック | タブ切替 |
| タブをダブルクリック | タブ名変更 |
| `+` をクリック | 新しいタブ |
| 境界をドラッグ | パネルのリサイズ |
| ホイールスクロール | ファイルツリー / プレビュー / ターミナル履歴。マウスレポートに opt-in している TUI (Claude Code `/tui fullscreen`, vim, lazygit, less 等) では代わりにアプリ側にホイールイベントが届く。 |
| ペイン内クリック / ドラッグ | 通常はテキスト選択 (コピー用)。マウスレポートに opt-in している TUI 上ではアプリにクリックが転送され、アプリ側のボタンやカーソル移動が動く。`Shift` を押しながらドラッグすると強制的に renga のテキスト選択 (tmux / alacritty と同じ escape hatch)。 |

ホイール / クリックいずれの転送も `RENGA_DISABLE_MOUSE_FORWARD=1` で無効化できます。renga をネストしている場合や、マウスプロトコル encoding が合わなくて内側アプリが混乱する環境向けの逃げ道です。

## IME overlay 内のキーマップ

IME 合成ボックスの **内側** で押すキー (送信、移動、閉じてもドラフト保持など) は [`ime.ja.md`](./ime.ja.md#overlay-内のキーマップ) を参照してください。本ページのペインモード表には、overlay を **開く** ための chord (`Ctrl+;` / `Alt+;` / `Alt+I`) だけを記載しています。
