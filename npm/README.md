# ccmux-fork

Claude Code Multiplexer (fork) — manage multiple Claude Code instances in TUI split panes.

> This is a fork of [Shin-sibainu/ccmux](https://github.com/Shin-sibainu/ccmux) published as `ccmux-fork`. It develops independent features while periodically syncing upstream.

## Install

```bash
npm install -g ccmux-fork
```

Migrating from the upstream `ccmux-cli`:

```bash
npm uninstall -g ccmux-cli && npm install -g ccmux-fork
```

## Usage

```bash
ccmux                    # Launch in current directory
ccmux /path/to/project   # Launch in specified directory
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
