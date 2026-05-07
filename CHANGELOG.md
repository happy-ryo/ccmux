# Changelog

All notable changes to renga are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and from v1.0 onward this project adheres to
[Semantic Versioning 2.0.0](https://semver.org/spec/v2.0.0.html) under the
rules in [`docs/semver-policy.md`](./docs/semver-policy.md).

## [Unreleased] — v1.0.0 (planned)

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
- **`spawn_codex_pane` now refuses to spawn when Codex's MCP config will
  not inject `RENGA_PEER_CLIENT_KIND=codex`.** Previously, if the user had
  not run `renga mcp install --client codex`, the freshly spawned codex
  pane registered as a `claude` (push) client and message delivery
  silently bifurcated. The handler now inspects `~/.codex/config.toml` for
  the `[mcp_servers.renga-peers.env] RENGA_PEER_CLIENT_KIND = "codex"`
  entry and fails the call with the new `[codex_not_installed]` error
  code, pointing the user at `renga mcp install --client codex`. The
  v1.0 freeze §6.2 entry tracking this as a follow-up has been removed
  and §1.8 / §5.1 are updated accordingly. Closes #203.

### Documentation

- Added a *Stability* section to `README.md` linking the freeze and policy
  docs.

## Pre-1.0 history

Pre-1.0 release notes are preserved in the version-history comments in
`Cargo.toml` and the GitHub Releases page. From v1.0 onward they will be
maintained here.
