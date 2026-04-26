# @suisya-systems/renga

Claude Code Multiplexer (fork) — manage multiple Claude Code instances in TUI split panes.

> This is a fork of [Shin-sibainu/ccmux](https://github.com/Shin-sibainu/ccmux) published as `@suisya-systems/renga` (previously `renga-fork`). It develops independent features while periodically syncing upstream.

## Install

```bash
npm install -g @suisya-systems/renga
```

Migrating from previous `renga-fork`:

```bash
npm uninstall -g renga-fork && npm install -g @suisya-systems/renga
```

Migrating from the upstream `renga-cli`:

```bash
npm uninstall -g renga-cli && npm install -g @suisya-systems/renga
```

## Usage

```bash
renga                    # Launch in current directory
renga /path/to/project   # Launch in specified directory
```

## Features

- Multi-pane terminal splits (vertical/horizontal)
- File tree sidebar with syntax-highlighted preview
- Tab workspaces
- Claude Code auto-detection (pane border turns orange)
- Mouse support (click, drag resize, text selection)
- Terminal scrollback (10,000 lines)
- Cross-platform (Windows, macOS, Linux)

## Links

- [GitHub (this fork)](https://github.com/happy-ryo/ccmux)
- [Upstream](https://github.com/Shin-sibainu/ccmux)

## License

MIT
