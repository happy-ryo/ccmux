# @suisya-systems/renga

A terminal multiplexer purpose-built for running multiple [Claude Code](https://docs.anthropic.com/en/docs/claude-code) sessions side-by-side — Claude-aware pane detection, peer messaging between Claude panes via a built-in MCP channel, and an IME-aware composition overlay for JP/CJK input.

For people running 2+ Claude Code instances in parallel (orchestrator + workers, side-by-side comparisons, etc.). If you only ever run one Claude at a time, the value over a plain terminal is small.

## Install

```bash
npm install -g @suisya-systems/renga
```

Migrating from previous `ccmux-fork`:

```bash
npm uninstall -g ccmux-fork && npm install -g @suisya-systems/renga
```

Migrating from the upstream `ccmux-cli`:

```bash
npm uninstall -g ccmux-cli && npm install -g @suisya-systems/renga
```

## Usage

```bash
renga                    # Launch in current directory
renga /path/to/project   # Launch in specified directory
```

## Features

- Peer messaging between Claude panes via the built-in `renga-peers` MCP channel
- Pane-control MCP tools (`spawn_claude_pane`, `set_pane_identity`, `new_tab`, `send_keys`, `inspect_pane`, ...)
- Centered IME composition overlay for JP / CJK input with pane freeze + draft restore on reopen
- Multi-pane splits, tab workspaces, and layout TOML
- File tree sidebar with syntax-highlighted preview
- Claude Code auto-detection (pane border turns orange)
- Mouse support (click, drag resize, text selection)
- Cross-platform (Windows, macOS, Linux)

## Links

- [GitHub](https://github.com/suisya-systems/renga)
- [Full README (with peer messaging, IME overlay, keybindings, configuration)](https://github.com/suisya-systems/renga#readme)

## History

renga was originally derived from [Shin-sibainu/ccmux](https://github.com/Shin-sibainu/ccmux) and has since evolved independently — peer messaging, IME overlay, layout TOML, and the bilingual UX layer are renga-specific. See [`BRANCHING.md`](https://github.com/suisya-systems/renga/blob/main/BRANCHING.md) for the divergence policy.

## License

MIT — upstream `Shin-sibainu/ccmux` copyright is retained per the license terms.
