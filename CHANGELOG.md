# Changelog

All notable changes to renga are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and from v1.0 onward this project adheres to
[Semantic Versioning 2.0.0](https://semver.org/spec/v2.0.0.html) under the
rules in [`docs/semver-policy.md`](./docs/semver-policy.md).

## [Unreleased]

## [1.1.3] — 2026-05-09

### Fixed

- **IME composition overlay: `Ctrl+Enter` now commits on WSL2 / Windows
  Terminal, where the host emulator binds `Alt+Enter` to *Toggle
  Fullscreen* and consumes the chord before renga sees it.** The host
  delivers the user's Ctrl+Enter as a bare LF byte (0x0A) when extended
  key reporting is off, which crossterm decodes into `Ctrl+J`; renga's
  overlay commit predicate (`is_overlay_commit_key`) now accepts
  `Ctrl+J` as the WSL fallback for Ctrl+Enter, so the buffer commits to
  the target pane via the existing bracketed-paste path. `Alt+Enter`
  remains the canonical commit binding on hosts that don't shadow it,
  and the existing `Ctrl+Enter` event path (used by terminals that opt
  into kitty keyboard protocol or xterm modifyOtherKeys) is unchanged.
  README, README.ja, and the docs site keymap tables now call out the
  WSL caveat next to the `Alt+Enter` row. (#226)

## [1.1.2] — 2026-05-09

Patch release. Bracketed-paste events now route to the IME composition
overlay when it holds focus, so on WSL2 / Windows Terminal / WezTerm a
Ctrl+V no longer leaks pasted text through to the back-pane PTY
(typically Claude Code's input row) while the user is composing in the
overlay. Pasted CRLF from Windows clipboards is normalized to LF inside
the overlay buffer so stray `\r` no longer renders as a zero-width
control glyph and desyncs the rendered cursor from the buffer cursor,
and the normalization streams through an iterator bounded by the
existing 4096-char overlay cap so a megabyte-class hostile paste no
longer briefly allocates a megabyte just to drop all but 4096 chars.
The frozen v1.0 API surface is unchanged — this is a routing bug fix in
keyboard input handling.

### Fixed

- **Bracketed-paste (`Event::Paste`) is now spliced into the IME
  composition overlay buffer at the cursor when the overlay is open,
  instead of being unconditionally forwarded to the back-pane PTY.**
  WSL2 / Windows Terminal / WezTerm surface Ctrl+V as a terminal-level
  bracketed-paste, so the previous unconditional forward leaked pasted
  text through to whatever foreground client owned the PTY (typically
  Claude Code's input row) even while the user was actively composing
  in the overlay. `App::handle_paste` now centralizes the routing
  decision: if `self.overlay` is open the paste is spliced into the
  composition buffer (honoring the existing 4096-char cap by truncating
  the tail — a clipped paste is recoverable, a dropped paste is not),
  otherwise the existing PTY path is used. The PTY-echo paste cooldown
  is skipped on the overlay branch since there's no PTY echo to wait
  for, so the overlay redraw fires immediately. Pasted CRLF / bare CR
  from Windows clipboards is normalized to LF inside the overlay
  buffer so the wrap/render path (which only treats `\n` as a hard
  newline) no longer renders stray `\r` as a zero-width control glyph
  and desyncs the rendered cursor from the buffer cursor by one char
  per line. The CRLF normalization streams through an
  `impl Iterator<Item = char>` bounded by `take(remaining)` against the
  overlay cap, so a megabyte-class hostile paste short-circuits the
  state machine the moment the cap is reached instead of allocating
  the full normalized string upfront just to drop all but 4096 chars.
  (#224)

## [1.1.1] — 2026-05-08

Patch release. Peer channel notifications now carry a loud
`📡 PEER MESSAGE … NOT FROM USER` banner so operators can tell at a
glance that a `Human:` turn is renga peer chatter rather than user
input, and `handle_peer_send` dedupes duplicate `(target, from, body)`
re-sends within a 5-second window so a chatty dispatcher no longer
produces two phantom turns on the receiving pane. Also relocates the
`spawn_codex_pane` verifier entry from `[1.0.0]` to here, since the
fix actually shipped in 1.1.1 (the v1.1.0 binary did not contain
PR #220). The frozen v1.0 API surface is unchanged — the banner is a
presentation tweak in `notifications/claude/channel` and the dedupe /
verifier are bug fixes.

### Changed

- **Peer channel notifications now lead with a `📡 PEER MESSAGE …
  NOT FROM USER` banner.** Claude Code injects renga peer messages
  into a user-slot turn, which the transcript renders under a
  `Human:` heading. The banner is wrapped around the body in
  `notifications/claude/channel` so an operator scanning a long
  transcript can tell at a glance that the line is peer chatter
  rather than something the human typed. The original body is
  preserved verbatim after the banner. (#221, #222)
- **`spawn_codex_pane` now refuses to spawn when Codex's MCP config will
  not inject `RENGA_PEER_CLIENT_KIND=codex`.** Previously, if the user had
  not run `renga mcp install --client codex`, the freshly spawned codex
  pane registered as a `claude` (push) client and message delivery
  silently bifurcated. The handler now inspects `~/.codex/config.toml` for
  the `[mcp_servers.renga-peers.env] RENGA_PEER_CLIENT_KIND = "codex"`
  entry and fails the call with the new `[codex_not_installed]` error
  code, pointing the user at `renga mcp install --client codex`. The
  v1.0 freeze §6.2 entry tracking this as a follow-up has been removed
  and §1.8 / §5.1 are updated accordingly. Closes #203. (#220)

### Fixed

- **`handle_peer_send` now drops duplicate `(target, from, body)`
  re-sends inside a 5-second window.** Previously a chatty
  dispatcher / worker that fired the same payload twice in quick
  succession (duplicate acks, `PR_MERGE_WATCH_TIMEOUT` false fires)
  produced two phantom `Human:` turns on the receiving Claude pane.
  The dedupe key includes the sender, so two distinct peers sending
  the same text still both deliver. (#221, #222)

## [1.1.0] — 2026-05-07

First release after the v1.0 API surface freeze. Two new optional features
ship without touching the frozen surface, alongside four bug fixes.

### Added

- **`--fps` CLI flag and `[ui] fps` config key** for tuning the main
  event-loop target rate. Higher values reduce idle input latency at the
  cost of more wakeups; `0` is accepted and clamped to `1` at runtime to
  avoid a busy loop. The CLI flag overrides the config key; default
  behavior is unchanged when neither is set. Adds `--fps` to the frozen
  CLI surface as a new optional flag and `[ui] fps` to the frozen config
  schema as a new optional key (#213).
- **Ctrl+U IME overlay shortcut** that discards the entire composition
  buffer in one keystroke (multi-line, not just the current line). Footer
  hint updated; lowercase / Shift / empty-buffer paths covered by tests
  (#211).

### Fixed

- **Self-targeted peer sends now emit `Event::PeerInbox` instead of being
  silently dropped.** `handle_peer_send` previously returned `Ok` without
  emitting the event when `target_id == from_pane`, so JSON-RPC reported
  `Delivered` while the recipient never observed the message. The
  self-send guard is removed; cross-tab silence (the actual security
  boundary) is preserved (#215, #217).
- **Pane close now walks the descendant process tree on Windows.**
  `portable-pty`'s `Child::kill` only terminates the immediate shell, so
  grandchildren (e.g. `claude` / `node.exe` started via
  `spawn_claude_pane`'s queued startup command) survived close and kept
  open handles on the pane's working directory, blocking
  `git worktree remove --force` and `Remove-Item`. `Pane::kill` now
  invokes `taskkill /F /T /PID <pid>` before delegating to portable-pty,
  short-circuits when `try_wait()` shows the child has already exited
  (avoids redundant taskkill from `Drop` after an explicit close), and
  always calls `wait()` so the child is reaped on every exit pathway —
  no more zombies on natural Unix exit (#214, #216).
- **Cosmetic Claude/Codex pane indicators latch across OSC title
  rewrites.** The per-pane border accent, pane label, and status-bar tab
  title now key off the sticky `claude_ever_seen()` / `codex_ever_seen()`
  latches instead of the live title check, since both clients rewrite
  their OSC 0/2 window titles to in-flight task summaries that frequently
  drop the literal client name. Foreground-app gating
  (`shell_accepts_command_injection`, mouse protocol resolution,
  `codex_peer` fallback) keeps using the live signal where "is the client
  foreground right now?" is actually needed.
  `pane_expects_codex_peer_delivery` short-circuits on a registered
  Claude pane so transient "codex" mentions in a Claude task title cannot
  mis-route delivery (#209, #210).
- **Cargo.toml version-history comments and BRANCHING.md no longer
  reference the legacy `ccmux` binary name.** Comments for 0.9.0,
  0.10.0, 0.13.0, 0.14.0, 0.16.0, 0.17.0, the interprocess pin comment,
  the `~/.config/ccmux/.macos_tip_dismissed` path, and the
  `CCMUX_NO_MACOS_TIP` env var are updated to current `renga` naming.
  BRANCHING.md "renga と ccmux" is qualified as "upstream ccmux" to
  disambiguate now that this repo IS renga (#212).

## [1.0.0] — 2026-05-02

API surface freeze release. Defines the v1.0 frozen surface and adopts
formal semver for all subsequent changes. The Cargo.toml / `npm/package.json`
version bump was omitted at tag time and is reconciled in 1.1.0; the v1.0.0
git tag and GitHub Release remain the canonical marker for this surface
freeze.

### Added

- **API surface freeze.** [`docs/api-surface-v1.0.md`](./docs/api-surface-v1.0.md)
  defines the v1.0 frozen surface across the four boundaries: MCP tools, CLI,
  IPC protocol, and config / layout / env.
- **Semver policy.** [`docs/semver-policy.md`](./docs/semver-policy.md)
  formalizes what counts as a breaking change, the deprecation window, and
  how additive changes ship.
- **`RENGA_TOKEN` / `RENGA_SOCKET` / `RENGA_PANE_ID` / `RENGA_PEER_CLIENT_KIND`**
  are now part of the formal v1.0 contract (previously de-facto stable).
- **MCP `serverInfo.name = "renga-peers"`** is now part of the frozen
  contract; downstream tools (Claude Code's channel-source tag) may rely on
  this string.
- **Detached-mode ok-text fallback prefixes** for `list_peers` and
  `send_message` are now part of the wire ABI.

### Changed

- **`set_summary` is now implemented (was stub).** The input shape is
  unchanged; the tool now stores a per-pane summary string in-memory and
  surfaces it as `summary` on every `PaneInfo` / `PeerInfo` returned by
  `list_panes` / `list_peers`. Empty input clears the summary; input
  longer than 256 Unicode scalar values is rejected with the new
  `[summary_too_long]` error code. Closes #202.

### Documentation

- Added a *Stability* section to `README.md` linking the freeze and policy
  docs.

## Pre-1.0 history

Pre-1.0 release notes are preserved in the version-history comments in
`Cargo.toml` and the GitHub Releases page. From v1.0 onward they will be
maintained here.
