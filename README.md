# ccmux (fork)

*Read this in other languages: [日本語](./README.ja.md)*

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

If you plan to send PRs, enable the repo's git hooks once after cloning:

```bash
git config core.hooksPath .githooks
```

This wires up a pre-commit hook that runs `cargo fmt --all -- --check` so a formatting miss fails locally instead of on CI. Opt-in so the setting never rewrites your existing `.git/hooks` without consent.

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
- `--lang <auto\|ja\|en>` — UI language for status bar hints and preview error messages (default `auto`). `auto` detects from the OS locale: `ja*` tags use Japanese, everything else falls back to English. `ja` / `en` force a specific language regardless of locale. Values are case-insensitive (`--lang JA` / `--lang En` both work). Also settable via `[ui] lang` in `config.toml`.

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
mode = "hotkey"   # "hotkey" | "off"
```

| Value | Behavior |
|-------|----------|
| `hotkey` (default) | `Ctrl+;` opens the IME composition overlay on a focused pane. `Alt+;` and `Alt+I` are fallbacks for terminals that swallow `Ctrl+;` (WSL under Windows Terminal, VS Code terminal on Linux, some tmux configs). |
| `off` | `Ctrl+;` is swallowed silently — no overlay, no keystroke leaked to the shell. For users who don't use IME, or whose terminal already handles IME placement correctly. |

The `--ime hotkey\|off` CLI flag overrides the config file for a single run. Precedence is **CLI > config file > default**.

> A third mode, `always`, used to auto-open the overlay on every Claude pane focus. It was removed because the auto-open never worked reliably in practice. Users who want the overlay ready on focus should just press `Ctrl+;` once.

### Recommended setup for JP / CJK IME users

If you regularly compose Japanese (or any IME-heavy language) prompts for Claude, launch ccmux with this pair:

```bash
ccmux --ime-freeze-panes --ime-overlay-catchup-ms 3000
```

Or set it once in `config.toml` so every session starts this way:

```toml
[ime]
freeze_panes_on_overlay = true
overlay_catchup_ms = 3000
```

Press `Ctrl+;` on a focused pane to open the overlay — the freeze + catch-up behavior kicks in automatically while the overlay is open.

![Centered IME overlay with a Japanese conversion candidate window anchored right under the caret, Claude panes frozen behind it](ime-overlay.png)

**What you get:**

1. **Overlay on demand.** Press `Ctrl+;` on a focused Claude pane — a centered multi-line composition box appears. The host-terminal IME candidate window anchors to the caret inside the box, so long JP words stop "jumping" around the screen mid-conversion (Issue #25).
2. **Pane flicker stops.** While the overlay is open, ccmux freezes the pane underneath — Claude's thinking spinner and streaming tokens no longer force repaints that would flicker past your IME candidates. You can focus entirely on composing.
3. **Progress stays visible.** Every 3 seconds, ccmux unfreezes for a single frame so you can see Claude's streamed output advance. Tune the interval with `--ime-overlay-catchup-ms`: `0` for pure freeze, `5000` if even 3 s feels busy.
4. **Multi-line drafts first-class.** `Enter` inserts a newline. Press `Alt+Enter` (macOS `Option+Return`) to send the whole buffer, or `Ctrl+Enter` on Windows Terminal / wezterm / VS Code. Full keymap is in the next subsection.
5. **Escape hatch.** `Esc` on the overlay closes it so you can use ccmux's pane-management shortcuts (`Ctrl+D` split, `Ctrl+Left/Right` focus cycle, etc.).

### IME overlay keybindings

The overlay opens as a centered multi-line composition box. Host-terminal IME candidate windows anchor to the caret inside the box.

| Key | Action |
|-----|--------|
| `Enter` | Insert newline (also `Shift+Enter`) |
| `Alt+Enter` | Send buffer to the pane and close (portable across all tier-1 terminals, incl. macOS `Option+Return`) |
| `Ctrl+Enter` | Send buffer — alternative commit for Windows Terminal / wezterm / VS Code / most Linux terminals |
| `Esc` / `Ctrl+C` | Cancel — closes the overlay and discards the buffer |
| `←` `→` `↑` `↓` | Navigate |
| `Home` / `End` | Start / end of current line |
| `Ctrl+Home` / `Ctrl+End` | Start / end of whole buffer |
| `Backspace` | Delete char left of caret |

### `[ui]` — UI language

Controls the language used for status bar hints and preview panel error messages. ccmux started out JP-only because the fork's primary users are Japanese speakers, but everything now flips automatically based on the OS locale.

```toml
[ui]
lang = "auto"   # "auto" | "ja" | "en"
```

| Value | Behavior |
|-------|----------|
| `auto` (default) | Detect from the OS locale via `sys-locale` (wraps `nl_langinfo` on Unix and `GetUserDefaultLocaleName` on Windows). Locales starting with `ja` render in Japanese; everything else falls back to English. Works even when `LANG` / `LC_*` are unset — handy for vanilla Windows Terminal + PowerShell. |
| `ja` | Force Japanese regardless of locale. |
| `en` | Force English regardless of locale. |

The `--lang auto\|ja\|en` CLI flag overrides the config file for a single run. Values are case-insensitive in both CLI (`--lang JA`) and TOML (`lang = "Ja"`). Precedence is **CLI > config > OS locale detection > English fallback**.

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
| `Ctrl+;` / `Alt+;` / `Alt+I` | Open IME composition overlay (centered multi-line — see below). `Alt+;` and `Alt+I` are fallbacks for terminals that swallow `Ctrl+;` (WSL under Windows Terminal, VS Code terminal on Linux, some tmux configs). |
| `Ctrl+Q` | Quit |

### File tree mode (after `Ctrl+F`)

| Key | Action |
|-----|--------|
| `j` / `k` | Move selection |
| `Enter` | Open file / expand directory (inline) |
| `h` | Move the tree root up one level |
| `l` | Descend into the selected directory (no-op on files) |
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
| Scroll wheel | Scroll file tree / preview / terminal history. In panes running a TUI that subscribed to mouse reporting (Claude Code `/tui fullscreen`, vim, lazygit, less, …) the wheel is forwarded to the app instead. |
| Click / drag inside a pane | Normally selects text for copy. When the pane is running a mouse-reporting TUI, the click is forwarded to the app so buttons, carets, etc. work. Hold `Shift` to force ccmux-side text selection (same escape hatch as tmux / alacritty). |

Both wheel and click forwarding can be disabled globally with `CCMUX_DISABLE_MOUSE_FORWARD=1` — useful for nested ccmux or terminals whose mouse-protocol encoding confuses the inner app.

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
