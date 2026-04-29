# renga Pitch Assets

## Positioning

`renga` is an AI-native terminal for teams running multiple coding agents in parallel.

It should be positioned as:

- The local command center for multi-agent software development
- A terminal substrate for agent orchestration, not a generic tmux replacement
- A mixed-client workspace where Claude Code and Codex can collaborate in-band

It should not be positioned as:

- "just another terminal multiplexer"
- "a better tmux"
- "a Claude-only niche utility"

## With `claude-org`

When paired with [`claude-org`](https://github.com/suisya-systems/claude-org), the product story becomes materially stronger.

`renga` alone is best understood as the execution fabric for local multi-agent development:

- agent-aware panes
- mixed-client peer messaging
- pane orchestration primitives
- local-first, terminal-native operation

`claude-org` adds the operational layer on top:

- Lead / Dispatcher / Curator / Worker role contracts
- narrow permission boundaries by role
- per-task working-directory discipline
- knowledge curation loops
- organization-level suspend / resume

Recommended stack message:

`renga` is the agent-native execution fabric. `claude-org` is the reference operating system for disciplined multi-agent development.

## Evaluation Shift: `renga` Alone vs `renga` + `claude-org`

| Axis | `renga` alone | `renga` + `claude-org` |
|---|---|---|
| Product category | Sharp AI-native terminal substrate | Full local multi-agent development stack |
| Value clarity | Strong for advanced users, but abstract to newcomers | Much easier to explain: one Lead, several Workers, explicit operating model |
| Buyer story | "better local coordination between agent panes" | "operational discipline for long-running multi-agent coding" |
| Competitive frame | Compared to `tmux`, `zellij`, Warp, Codex app | Compared to Agent Teams, ccswarm, agent farms, manual multi-agent ops |
| Strength | Mixed-client pane orchestration and local execution | End-to-end workflow: roles, permissions, knowledge loop, state handling |
| Weakness | Can look like an excellent component without a complete use case | Higher setup and conceptual overhead; more moving parts |
| Reliability burden | Falls heavily on transport, pane state, and UX polish | Shared across stack layers; framework can absorb some workflow complexity |
| Market perception | "tool for power users" | "reference system for serious multi-agent work" |
| Defensibility | Good primitives, but easier to dismiss as niche | Stronger system story because the lower layer is justified by a real operating model |

## Evaluation Shift: Strengths and Weaknesses

### What improves when `claude-org` is present

- The need for `renga` becomes concrete rather than theoretical.
- `renga-peers`, `spawn_claude_pane`, `inspect_pane`, and `send_keys` are no longer just clever tools; they become required primitives for a working organization runtime.
- The stack can credibly claim an opinionated alternative to both naive `tmux` splits and high-parallelism agent farms.
- The messaging upgrades from "AI-aware terminal" to "disciplined AgentOps stack for local development."

### What remains weak or becomes newly important

- The top-level stack is still Claude-first today; `renga` supports mixed-client collaboration, but `claude-org` explicitly centers Claude Code and treats Codex as optional.
- Onboarding cost rises. The system is more valuable, but also more demanding.
- Clear layer boundaries matter more. If Layer 2 / Layer 1 extraction stays vague for too long, the stack can feel integrated but not yet modular.

## Priority Shift in Required Investments

`renga` alone suggests one priority order; `renga` under `claude-org` changes that order slightly.

### Priority if `renga` is evaluated alone

1. Mixed-client reliability
2. Session persistence
3. Worktree-aware worker workflows

### Priority if `renga` is evaluated as part of the `claude-org` stack

1. Worktree-aware worker workflows  
Per-task directory and isolation discipline become central, not optional.

2. Mixed-client reliability  
Still strategically important, but less immediate if the core production path remains Claude-first.

3. Session persistence  
Still useful, but somewhat buffered by `claude-org`'s organization-level suspend / resume model.

## Landing Page Hero Copy

### Option A

**Headline**  
The AI-native terminal for agent teams.

**Subheadline**  
Run Claude Code and Codex side by side, let them message each other, and orchestrate workers from inside the terminal itself.

### Option B

**Headline**  
Your local command center for multi-agent coding.

**Subheadline**  
`renga` turns terminal panes into first-class agent endpoints, with mixed-client peer messaging, pane orchestration, and CJK-friendly input that still works under real development load.

### Option C

**Headline**  
Where coding agents become a team.

**Subheadline**  
Stop copy-pasting between AI sessions. Launch, inspect, coordinate, and steer multiple Claude Code and Codex workers in one workspace.

## Three-Minute Pitch

Most developer tools still assume one human and one terminal session. Even most AI coding tools assume one user talking to one model at a time.

That is already outdated.

Serious developers are starting to work with multiple coding agents in parallel. One agent explores a bug, another writes a patch, another reviews a subsystem, and a lead agent coordinates the work. The problem is that today's terminal tools do not understand that workflow. `tmux` and `zellij` understand panes, but not agents. Vendor apps understand one agent well, but they usually trap you inside one client or one UX model.

`renga` is the missing layer between those worlds.

It is an AI-native terminal where panes are treated as first-class agent endpoints. In one tab, you can run Claude Code and Codex side by side, let them exchange structured peer messages, launch new workers, inspect another pane's visible screen, answer interactive prompts, and watch worker lifecycle events without leaving the conversation.

This matters because multi-agent development breaks down when coordination is manual. If the human has to relay every instruction, copy every error, and babysit every prompt, the productivity gains disappear. `renga` keeps that coordination in-band and local.

It also solves a problem most global developer tools ignore: non-English input. On Windows and third-party terminals, Japanese, Chinese, and Korean IME input often becomes unusable while fast-moving TUIs stream output. `renga` ships a centered composition overlay that keeps the IME anchored and makes multilingual prompting practical.

So the wedge is clear: `renga` is not trying to replace every terminal workflow. It is trying to become the best local workspace for developers who already use multiple coding agents seriously.

The long-term opportunity is larger than a multiplexer. If agent-based development becomes normal, developers will need a local execution and coordination layer for agent teams. `renga` can become that layer.

If paired with `claude-org`, the story gets even stronger: the market is no longer evaluating a terminal primitive in isolation, but a full operating model built on top of it.

## Investor One-Pager

### Problem

Developers are beginning to use multiple coding agents in parallel, but current tools are fragmented:

- traditional multiplexers manage shells, not agents
- AI apps optimize for one agent at a time
- cross-agent coordination still depends on manual copy-paste
- multilingual developers suffer from broken IME behavior in terminal AI workflows

### Product

`renga` is an AI-native terminal substrate for orchestrating multiple coding agents in one workspace.

Core product primitives:

- mixed-client peer messaging between Claude Code and Codex
- pane lifecycle and layout orchestration through MCP tools
- local, tab-scoped coordination instead of heuristic peer discovery
- multilingual IME-safe prompting for JP/CJK users
- lightweight single-binary deployment across Windows, macOS, and Linux

### Why Now

- coding agents are moving from novelty to daily workflow
- developers increasingly run multiple agents for research, implementation, and review
- existing terminal and IDE abstractions were built before multi-agent workflows mattered

### Why This Team/Product Has an Edge

- built around real operator workflows instead of generic terminal abstractions
- mixed-client architecture avoids betting on a single model vendor
- strong wedge in underserved global developer UX, especially CJK input
- local-first design keeps setup simple and latency low

### Risks

- the current user persona is narrow: advanced developers already using 2+ agents
- mixed-client reliability, especially Codex coordination, must improve
- session persistence and worktree-aware workflows are not mature yet

### Required Investments

1. Mixed-client reliability  
Make Claude/Codex coordination predictable enough for daily use, especially around delivery guarantees, approval friction, and visible state.

2. Session persistence  
Restore layouts, pane identity, and team state so long-running agent work survives restarts.

3. Worktree-aware team workflows  
Let orchestrators spin up isolated workers against separate worktrees or task directories without hand-built shell glue.

### Vision

The terminal is becoming the operating surface for software agents. `renga` aims to be the local system that turns many agents into one coordinated team.
