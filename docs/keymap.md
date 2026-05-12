# Keymap

Full keybinding reference. The README only carries a "first 5–8 keys" cheat sheet; everything else lives here.

> **macOS users:** the default macOS terminal swallows `Option+<key>` before renga sees it, so `Alt+T`, `Alt+P`, `Alt+1..9`, `Alt+Left/Right` etc. won't fire out of the box. See [macOS: Option as Meta](#macos-option-as-meta) below for the one-line fix per terminal.

## Pane mode (default)

| Key | Action |
|-----|--------|
| `Ctrl+D` | Split vertically |
| `Ctrl+E` | Split horizontally |
| `Ctrl+W` | Close pane / tab |
| `Alt+T` / `Ctrl+T` | New tab |
| `Alt+1..9` | Jump to tab N |
| `Alt+Left/Right` | Previous / next tab |
| `Alt+R` | Rename tab (session only) |
| `Alt+S` | Toggle status bar |
| `Alt+P` | Insert the peer-enabled Claude Code launch command into the focused pane (see [`peer-messaging.md`](./peer-messaging.md)). |
| `Ctrl+F` | Toggle file tree |
| `Ctrl+P` | Swap preview/terminal layout |
| `Ctrl+Right/Left` | Cycle focus (sidebar, preview, panes) |
| `Ctrl+;` / `Alt+;` / `Alt+I` | Open IME composition overlay (centered multi-line — see [`ime.md`](./ime.md)). `Alt+;` and `Alt+I` are fallbacks for terminals that swallow `Ctrl+;` (WSL under Windows Terminal, VS Code terminal on Linux, some tmux configs). |
| `Ctrl+Q` | Quit |

## macOS: Option as Meta

By default macOS terminals bind `Option+<key>` to Unicode input (`å`, `∫`, `π`, …), so renga's `Alt+T` / `Alt+P` / `Alt+R` / `Alt+S` / `Alt+1..9` / `Alt+Left/Right` shortcuts never reach the app. Flip Option to act as a Meta key — it's a one-line change in every modern terminal. If you're on plain **Terminal.app**, consider switching to one of the terminals below first; they all handle IME, ligatures, and the image preview panel better than Terminal.app anyway.

| Terminal | Setting |
|---|---|
| **WezTerm** (`~/.wezterm.lua`) | `config.send_composed_key_when_left_alt_is_pressed = false` <br> `config.send_composed_key_when_right_alt_is_pressed = false` |
| **iTerm2** | Settings → Profiles → Keys → set **Left Option key** and **Right Option key** to **Esc+** |
| **Alacritty** (`~/.config/alacritty/alacritty.toml`) | `[window]` <br> `option_as_alt = "Both"` (or `"OnlyLeft"` / `"OnlyRight"`) |
| **Ghostty** (`~/.config/ghostty/config`) | `macos-option-as-alt = true` |
| **Kitty** (`~/.config/kitty/kitty.conf`) | `macos_option_as_alt yes` |
| **Terminal.app** | Settings → Profiles → Keyboard → tick **Use Option as Meta key** |

**Known gaps**

- Some macOS IMEs (Kotoeri's "Romaji" toggle, kana layouts, …) bind Option themselves. If flipping Option breaks IME for you, try the `OnlyLeft` / `OnlyRight` variants so one Option stays native to the OS.
- `Alt+1..9` can collide with macOS Mission Control / Spaces shortcuts on some setups. If the OS swallows the number keys, `Alt+Left/Right` still cycles tabs.

## File tree mode (after `Ctrl+F`)

| Key | Action |
|-----|--------|
| `j` / `k` | Move selection |
| `Enter` | Open file / expand directory (inline) |
| `h` | Move the tree root up one level |
| `l` | Descend into the selected directory (no-op on files) |
| `c` | Split left/right and queue the Claude+peer launch line in the selection's directory (file → parent, empty → tree root). Not executed until you press Enter, same as `Alt+P`. |
| `v` | Same as `c` but splits top/bottom. |
| `.` | Toggle hidden files |
| `Esc` | Return to pane |

## Preview mode (after focusing preview)

| Key | Action |
|-----|--------|
| `j` / `k` | Scroll vertically |
| `h` / `l` | Scroll horizontally |
| `Ctrl+W` | Close preview |
| `Esc` | Return to pane |

## Mouse

| Action | Effect |
|--------|--------|
| Click pane | Focus pane |
| Click tab | Switch tab |
| Double-click tab | Rename tab |
| Click `+` | New tab |
| Drag border | Resize panels |
| Scroll wheel | Scroll file tree / preview / terminal history. In panes running a TUI that subscribed to mouse reporting (Claude Code `/tui fullscreen`, vim, lazygit, less, …) the wheel is forwarded to the app instead. |
| Click / drag inside a pane | Normally selects text for copy. When the pane is running a mouse-reporting TUI, the click is forwarded to the app so buttons, carets, etc. work. Hold `Shift` to force renga-side text selection (same escape hatch as tmux / alacritty). |

Both wheel and click forwarding can be disabled globally with `RENGA_DISABLE_MOUSE_FORWARD=1` — useful for nested renga or terminals whose mouse-protocol encoding confuses the inner app.

## IME overlay keymap

The keys *inside* the IME composition box (commit, navigate, close-and-restore-draft, etc.) are documented in [`ime.md`](./ime.md#overlay-keymap). Only the open / fallback chords (`Ctrl+;` / `Alt+;` / `Alt+I`) appear in the Pane-mode table above.
