# ccmux (fork)

Claude Code Multiplexer — manage multiple Claude Code instances in TUI split panes.

A lightweight terminal multiplexer built specifically for running multiple [Claude Code](https://docs.anthropic.com/en/docs/claude-code) sessions side-by-side.

> **This is a fork** of [Shin-sibainu/ccmux](https://github.com/Shin-sibainu/ccmux) that develops independent features while periodically syncing upstream. Installs as the separate npm package `ccmux-fork`. See [`BRANCHING.md`](./BRANCHING.md) for the fork policy.

![ccmux screenshot](screenshot.png)

## Features

- **Multi-pane terminal** — Split vertically/horizontally, run independent PTY shells
- **Tab workspaces** — Multiple project tabs with click-to-switch
- **File tree sidebar** — Browse project files with icons, expand/collapse directories
- **Syntax-highlighted preview** — View file contents with language-aware coloring
- **Claude Code detection** — Pane border turns orange when Claude Code is running
- **cd tracking** — File tree and tab name auto-update when you change directories
- **Mouse support** — Click to focus, drag borders to resize, scroll history
- **Scrollback** — 10,000 lines of terminal history per pane
- **Dark theme** — Claude-inspired color scheme
- **Cross-platform** — Windows, macOS, Linux
- **Single binary** — ~1MB, no runtime dependencies

## Install

### Via npm (recommended)

```bash
npm install -g ccmux-fork
```

> Previously installed the upstream `ccmux-cli`? Migrate with: `npm uninstall -g ccmux-cli && npm install -g ccmux-fork`

### Download binary

Download the latest binary from [Releases](https://github.com/happy-ryo/ccmux/releases):

| Platform | File |
|----------|------|
| Windows (x64) | `ccmux-windows-x64.exe` |
| macOS (Apple Silicon) | `ccmux-macos-arm64` |
| macOS (Intel) | `ccmux-macos-x64` |
| Linux (x64) | `ccmux-linux-x64` |

> **Windows:** Microsoft Defender SmartScreen may show a warning because the binary is not code-signed. Click "More info" → "Run anyway" to proceed. This is normal for unsigned open-source software.

> **macOS/Linux:** After downloading, make the binary executable: `chmod +x ccmux-*`

### From source

```bash
git clone https://github.com/happy-ryo/ccmux.git
cd ccmux
cargo build --release
# Binary at target/release/ccmux (or ccmux.exe on Windows)
```

Requires [Rust](https://rustup.rs/) toolchain.

## Usage

```bash
ccmux
```

Launch from any directory. The file tree shows the current working directory.

### Flags

- `--min-pane-width <COLS>` — minimum child columns a split may produce (default `20`). Splits whose halved pane would be narrower are refused. `0` is clamped to `1` to avoid zero-width children.
- `--min-pane-height <ROWS>` — minimum child rows a split may produce (default `5`). Same clamp rule as `--min-pane-width`.
- `--ime-freeze-panes[=BOOL]` — freeze pane repaints while the IME composition overlay is open (default `false`). Suppresses flicker from Claude's thinking spinner and other background PTY output during JP composition; panes catch up instantly when the overlay closes. Pass the bare flag to enable, or `=false` to force-disable a config `true`. Also settable via `[ime] freeze_panes_on_overlay` in `config.toml`.
- `--ime-overlay-catchup-ms <MS>` — when `--ime-freeze-panes` is active, force a single repaint every `<MS>` milliseconds so body-content progress stays visible through an open overlay (default `0` = disabled, pure freeze). `3000`–`5000` is the sweet spot: flicker stays barely noticeable while Claude's streaming output still advances at a readable pace. Non-zero values are clamped to at least `100`. Also settable via `[ime] overlay_catchup_ms` in `config.toml`.

## Configuration

Optional. Place a TOML file at:

- **Linux**: `$XDG_CONFIG_HOME/ccmux/config.toml` (default `~/.config/ccmux/config.toml`)
- **macOS**: `~/Library/Application Support/ccmux/config.toml`
- **Windows**: `%APPDATA%\ccmux\config.toml`

Missing or malformed files fall back to defaults with a stderr warning; ccmux never fails to start because of a config issue. Unknown sections and keys are ignored for forward-compat.

### `[ime]` — IME composition overlay

Controls the IME overlay used for host-terminal IME input (Issue #25 / PR #36).

```toml
[ime]
mode = "hotkey"   # "hotkey" | "off" | "always"
```

| Value | Behavior |
|-------|----------|
| `hotkey` (default) | `Ctrl+;` opens the IME composition overlay on a focused pane. |
| `off` | `Ctrl+;` is swallowed silently — no overlay, no keystroke leaked to the shell. For users who don't use IME, or whose terminal already handles IME placement correctly. |
| `always` | The overlay is opened automatically whenever focus rests on a non-scrolled Claude pane, so IME composition (including JP) has an anchor from the first keystroke. Press `Esc` (with an empty buffer) or `Ctrl+C` to dismiss the overlay and interact with the pane directly — the dismiss key is forwarded to the pane so Claude's Esc-to-interrupt still works. Moving focus to another pane and back re-opens the overlay. A printable key on a dismissed overlay still triggers auto-open as a half-width shortcut; scrolled-back panes and shell panes never auto-open. **Tradeoff:** while the overlay is open, ccmux pane-management shortcuts (Ctrl+D split, Ctrl+Left/Right focus-cycle, Alt+Left/Right tab-nav, etc.) do not fire — dismiss first, then use them. If that friction is unwanted, stay on `hotkey` and press Ctrl+; only when you need IME. |

The `--ime hotkey|off|always` CLI flag overrides the config file for a single run. Precedence is **CLI > config file > default**.

### Recommended setup for JP / CJK IME users

If you regularly compose Japanese (or any IME-heavy language) prompts for Claude, launch ccmux with this trio:

```bash
ccmux --ime always --ime-freeze-panes --ime-overlay-catchup-ms 3000
```

Or set it once in `config.toml` so every session starts this way:

```toml
[ime]
mode = "always"
freeze_panes_on_overlay = true
overlay_catchup_ms = 3000
```

![Centered IME overlay composing Japanese — candidate window anchors right under the caret, panes stay frozen behind it](ime-overlay.png)

**What you get:**

1. **Overlay opens automatically.** As soon as focus lands on a Claude pane, a centered multi-line composition box appears. The host-terminal IME candidate window anchors to the caret inside the box, so long JP words stop "jumping" around the screen mid-conversion (Issue #25).
2. **Pane flicker stops.** While the overlay is open, ccmux freezes the pane underneath — Claude's thinking spinner and streaming tokens no longer force repaints that would flicker past your IME candidates. You can focus entirely on composing.
3. **Progress stays visible.** Every 3 seconds, ccmux unfreezes for a single frame so you can see Claude's streamed output advance. Tune the interval with `--ime-overlay-catchup-ms`: `0` for pure freeze, `5000` if even 3 s feels busy.
4. **Multi-line drafts first-class.** `Enter` inserts a newline. Press `Alt+Enter` (macOS `Option+Return`) to send the whole buffer, or `Ctrl+Enter` on Windows Terminal / wezterm / VS Code. Full keymap is in the next subsection.
5. **Escape hatch.** `Esc` on an empty buffer closes the overlay so you can use ccmux's pane-management shortcuts (`Ctrl+D` split, `Ctrl+Left/Right` focus cycle, etc.); moving focus to another pane and back re-opens it.

If the always-open behavior feels intrusive, swap `mode = "always"` for `mode = "hotkey"` — the overlay then only opens when you press `Ctrl+;`, but the other two flags still eliminate flicker when it is open.

### IME overlay keybindings

The overlay opens as a centered multi-line composition box. Host-terminal IME candidate windows anchor to the caret inside the box.

| Key | Action |
|-----|--------|
| `Enter` | Insert newline (also `Shift+Enter`) |
| `Alt+Enter` | Send buffer to the pane and close (portable across all tier-1 terminals, incl. macOS `Option+Return`) |
| `Ctrl+Enter` | Send buffer — alternative commit for Windows Terminal / wezterm / VS Code / most Linux terminals |
| `Esc` / `Ctrl+C` | Cancel. Empty buffer closes the overlay; non-empty buffer in Always mode first clears, second press dismisses |
| `←` `→` `↑` `↓` | Navigate |
| `Home` / `End` | Start / end of current line |
| `Ctrl+Home` / `Ctrl+End` | Start / end of whole buffer |
| `Backspace` | Delete char left of caret |

## Keybindings

### Pane mode (default)

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
| `Ctrl+F` | Toggle file tree |
| `Ctrl+P` | Swap preview/terminal layout |
| `Ctrl+Right/Left` | Cycle focus (sidebar, preview, panes) |
| `Ctrl+;` | Open IME composition overlay (centered multi-line — see below) |
| `Ctrl+Q` | Quit |

### File tree mode (after `Ctrl+F`)

| Key | Action |
|-----|--------|
| `j` / `k` | Move selection |
| `Enter` | Open file / expand directory |
| `.` | Toggle hidden files |
| `Esc` | Return to pane |

### Preview mode (after focusing preview)

| Key | Action |
|-----|--------|
| `j` / `k` | Scroll vertically |
| `h` / `l` | Scroll horizontally |
| `Ctrl+W` | Close preview |
| `Esc` | Return to pane |

### Mouse

| Action | Effect |
|--------|--------|
| Click pane | Focus pane |
| Click tab | Switch tab |
| Double-click tab | Rename tab |
| Click `+` | New tab |
| Drag border | Resize panels |
| Scroll wheel | Scroll file tree / preview / terminal history |

## Architecture

```
src/
├── main.rs       # Entry point, event loop, panic hook
├── app.rs        # Workspace/tab state, layout tree, key/mouse handling
├── pane.rs       # PTY management, vt100 emulation, shell detection
├── ui.rs         # ratatui rendering, theme, layout
├── filetree.rs   # File tree scanning, navigation
└── preview.rs    # File preview with syntax highlighting
```

**Key design decisions:**
- `vt100` crate for terminal emulation (not ANSI stripping) — needed for Claude Code's interactive UI
- Binary tree layout for recursive pane splitting with variable ratios
- Per-PTY reader threads with mpsc channel to main event loop
- OSC 7 detection for automatic cd tracking
- Dirty-flag rendering for minimal CPU usage when idle

## Tech Stack

- [ratatui](https://ratatui.rs/) + [crossterm](https://github.com/crossterm-rs/crossterm) — TUI framework
- [portable-pty](https://github.com/nickelc/portable-pty) — PTY abstraction (ConPTY on Windows)
- [vt100](https://crates.io/crates/vt100) — Terminal emulation
- [syntect](https://github.com/trishume/syntect) — Syntax highlighting

## Learn Claude Code

New to Claude Code? Check out [Claude Code Academy](https://claude-code-academy.dev) for tutorials and guides.

## License

MIT
