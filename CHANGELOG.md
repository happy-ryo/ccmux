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

### Documentation

- Added a *Stability* section to `README.md` linking the freeze and policy
  docs.

## Pre-1.0 history

Pre-1.0 release notes are preserved in the version-history comments in
`Cargo.toml` and the GitHub Releases page. From v1.0 onward they will be
maintained here.
