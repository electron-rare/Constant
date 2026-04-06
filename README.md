# Constant

```text
 ██████╗ ██████╗ ███╗   ██╗███████╗████████╗ █████╗ ███╗   ██╗████████╗
██╔════╝██╔═══██╗████╗  ██║██╔════╝╚══██╔══╝██╔══██╗████╗  ██║╚══██╔══╝
██║     ██║   ██║██╔██╗ ██║███████╗   ██║   ███████║██╔██╗ ██║   ██║
██║     ██║   ██║██║╚██╗██║╚════██║   ██║   ██╔══██║██║╚██╗██║   ██║
╚██████╗╚██████╔╝██║ ╚████║███████║   ██║   ██║  ██║██║ ╚████║   ██║
 ╚═════╝ ╚═════╝ ╚═╝  ╚═══╝╚══════╝   ╚═╝   ╚═╝  ╚═╝╚═╝  ╚═══╝   ╚═╝

      local-first orchestration for weird builders, fleet nerds, and AI cockpit addicts
```

`Constant` is a control room.

Not a chatbot skin.
Not a SaaS wrapper.
Not another “agent framework” that only looks good in slides.

It is a pragmatic, local-first orchestration layer that sits on top of:

- a 4-pane `tmux` machine cockpit
- a multi-machine fleet view
- host-local CLIs like `claude`, `codex`, `copilot`, and `vibe`
- a mission runner that can route work across machines
- a growing durable memory layer for repo context, decisions, and persona

The vibe is somewhere between:

- demoscene terminal
- distributed workshop
- AI pit crew
- `electron rare` engineering energy

## What Ships Today

Current build status in this repo:

- a Rust front-controller for the canonical `constant` entrypoint
- a 4-pane host-local `tmux` session per machine
- a fleet cockpit with one window per machine plus a central `Constant` window
- `Constant`, the orchestration CLI on top of the cockpit
- a terminal TUI with a central `hexapus` buddy rail
- live cockpit controls for focus, capture, send, and pane restart
- local mission planning and verification
- host-local execution for `claude`, `codex`, and `vibe`
- `copilot` as a manual lane
- local message bus + cross-machine bridge
- MLX-ready model plumbing for a small local orchestrator stack on macOS
- workspace-first durable memory with lexical + local vector search

The canonical script surface is `constant-*`.
Legacy `zellij-ai-*` aliases remain only as compatibility shims.

Migration note:

- the entrypoint is now moving to Rust
- `doctor`, `agents`, `skills`, `tui`, `cockpit`, `mission create`, `mission plan`, `mission run`, `mission status`, `mission tail`, `mission verify`, `mission retry`, `mission summarize`, and `delegate` now run from the Rust binary
- Rust now prefers `fleet.toml`, `models.toml`, and `memory.toml`, while keeping JSON mirrors for compatibility during migration
- `memory`, `buddy`, and `fleet` still hand off to the existing Python core during migration
- the public `./scripts/Constant` wrapper now tries the Rust binary first, but runs a fast startup probe before handing control over
- if macOS stalls in code-signing / policy evaluation for freshly built binaries, the wrapper falls back automatically to the stable Python path
- when `constant` is launched from inside the Codex runner itself, the wrapper skips the Rust path entirely and goes straight to Python to avoid the known macOS startup stall in that environment
- use `CONSTANT_USE_RUST=1 ./scripts/Constant ...` to force the raw Rust path, or `CONSTANT_USE_PYTHON=1 ./scripts/Constant ...` to force Python
- after changing macOS security settings, use `CONSTANT_RUST_RECHECK=1 ./scripts/Constant ...` to force a fresh Rust startup probe instead of waiting for the fallback cache TTL

macOS note:

- if Rust keeps falling back from a normal terminal app, check `System Settings -> Privacy & Security -> Developer Tools`
- allow your terminal app there so it can run locally built binaries without extra security-policy stalls
- then rerun with `CONSTANT_RUST_RECHECK=1 ./scripts/Constant --help`

## Why This Exists

Most AI tooling is designed like a vending machine:

1. send a prompt
2. get an answer
3. pretend orchestration happened

That breaks the moment you want:

- multiple machines
- multiple CLIs
- persistent sessions
- human supervision without tab hell
- routing by task type
- repo memory that survives a single run

`Constant` is for the opposite use case:

- the workstation is real
- the shell is real
- the repos are real
- the agents are messy
- the human is still the one in charge

## Core Shape

### 1. Machine cockpit

Each machine runs the same 4-pane layout:

- left: `claude`
- top-right: `codex`
- middle-right: `copilot`
- bottom-right: `vibe`

Everything runs on the host by default.
No Docker dependency is required for the standard session.

### 2. Fleet cockpit

From your command center machine, you open one `tmux` window per machine plus a central `Constant` window.
Each machine window attaches to that machine’s own local `tmux` session.

That gives you:

- one place to supervise the whole fleet
- one active orchestration surface in the `Constant` window
- local clipboard behavior on the operator machine
- remote sessions that stay remote
- a clean separation between orchestration and execution

### 3. Constant CLI

`Constant` sits above the fleet and handles:

- mission creation
- route selection
- backend selection
- CLI selection
- buddy review
- verification
- delegation
- cockpit handoff
- SSH discovery and fleet deployment bootstrap

### 3.5. Workflow skills

`Constant` now has a first-class workflow skill layer.

Public workflow skills:

- `spec-planner`
- `architecture-brainstorm`
- `repo-onboarding`
- `task-decomposer`
- `pr-review-prep`

Recommended minimal stack:

- `spec-planner`
- `architecture-brainstorm`
- `task-decomposer`

Recommended workflow:

1. `spec-planner`
2. `architecture-brainstorm`
3. `task-decomposer`
4. implementation on the selected agent / machine / CLI
5. `pr-review-prep`

Inspect them with:

```bash
./scripts/Constant skills --json
./scripts/Constant skills --public-only --json
./scripts/Constant agents --json
```

Create and route missions through these skills today with the Rust CLI:

```bash
./scripts/Constant mission create "Je veux ajouter un systeme de notifications" --workspace "$PWD"
./scripts/Constant delegate <mission_id> --skill architecture-brainstorm --json
```

The richer conversation-first chat surface is still part of the Python-side migration path.

### 4. Durable memory

The memory layer is being built to support:

- workspace-first indexing
- weighted instruction fusion from `.claude`, `.copilot`, `.agent`, `.agents`, `CLAUDE.md`, `AGENTS.md`
- cross-mission summaries
- a durable decision graph
- persistent persona outside mission files

The design goal is simple:
`Constant` should remember how you work, not just what you typed five minutes ago.

## Quick Start

### Requirements

- `tmux`
- `git`
- `node`
- `npm`
- `uv`
- `claude`
- `codex`
- `copilot`
- `vibe`

Recommended CLI install channels:

```bash
npm install -g @anthropic-ai/claude-code
npm install -g @openai/codex
npm install -g @github/copilot
uv tool install mistral-vibe
```

### Start the cockpit

```bash
./scripts/constant-machine.sh --workspace "$PWD"
```

### Start the fleet cockpit

```bash
./scripts/constant-fleet.sh --workspace "$PWD"
```

For non-interactive setup or tests, you can create the sessions without attaching:

```bash
./scripts/constant-machine.sh --workspace "$PWD" --ensure-only
./scripts/constant-fleet.sh --workspace "$PWD" --ensure-only
```

On first launch of the Codex pane, if `auth.json` is missing in its profile, the pane runs:

```bash
codex login --device-auth
```

### Start Constant

```bash
./scripts/Constant
./scripts/Constant tui
./scripts/Constant doctor
./scripts/Constant cockpit doctor --json
./scripts/Constant cockpit status --json
./scripts/Constant cockpit focus --machine command-center --pane codex
./scripts/Constant cockpit capture --machine command-center --pane claude
./scripts/Constant mission create "audit the repo" --workspace "$PWD"
./scripts/Constant mission status
./scripts/Constant memory rebuild --workspace "$PWD"
./scripts/Constant memory search "buddy rail" --workspace "$PWD"
./scripts/Constant cockpit open --workspace "$PWD"
./scripts/Constant fleet discover --json
./scripts/Constant fleet configure
./scripts/Constant fleet deploy
```

If `Constant` is on your `PATH`, you can also just run:

```bash
Constant
```

In interactive mode, `Constant` with no arguments now opens or attaches the full fleet cockpit by default.

That means:

- a top tmux tab bar for `Constant`, the local machine, and each remote machine
- the active tab is forced back to `Constant` when you launch `constant`
- the `Constant` TUI inside the central `Constant` window
- one machine window per host in the fleet

For now, the central `Constant` window is intentionally forced onto the richer Python chat-first TUI so the UX stays closer to a Claude Code-style conversation surface while the Rust TUI is still catching up.

The current interaction model is:

- chat-first by default
- prompt focused on launch
- `/` opens the workflow skill palette
- `Esc` drops into cockpit command mode
- `f` toggles between `chat-focus` and `cockpit-detail`

If you want only the standalone TUI without attaching the fleet session, use:

```bash
./scripts/Constant tui --workspace "$PWD"
```

### Discover and deploy a fleet

`Constant` now ships with a public discovery/deployment CLI that:

- scans `~/.ssh/config`
- scans `~/.ssh/known_hosts`
- scans local `arp -a` neighbors
- validates candidates with a short SSH probe
- lets you select targets interactively
- asks for SSH user and machine labels
- writes `~/.config/constant/fleet.toml` and a compatibility `fleet.json` mirror
- can immediately deploy the runtime to the selected machines

Examples:

```bash
./scripts/constant-deploy.sh scan --json
./scripts/constant-deploy.sh configure
./scripts/constant-deploy.sh deploy

Constant fleet discover --json
Constant fleet configure --host dev@builder-a --host dev@edge-a
Constant fleet deploy --repo-dir '$HOME/constant'
```

You can pass raw SSH seeds such as:

- `dev@builder-a`
- `root@192.168.0.119`
- `lab-a`

If `fleet.toml` or `fleet.json` contains `repo_dir`, the shell launchers and fleet installer will reuse it automatically.

For a fully non-interactive run, pass explicit hosts and `--yes`:

```bash
Constant fleet configure \
  --host builder-a \
  --host builder-b \
  --user dev \
  --repo-dir '$HOME/constant' \
  --local-label command-center \
  --yes
```

### TUI keys

`Constant tui` now gives you:

- a chat-first central conversation surface
- a thread sidebar
- a chat-focus mode that behaves closer to Claude Code
- a detailed cockpit view you can toggle back in when needed
- a capture popup for pane output
- a `hexapus` buddy rail
- a bottom status / key strip

Useful keys:

- `j` / `k`: move between missions
- `f`: toggle `chat-focus` / `cockpit-detail`
- `[` / `]`: move between machines in the runtime view
- `1`..`4`: select `claude`, `codex`, `copilot`, `vibe` on the selected machine
- `o`: focus the selected machine + pane in the real cockpit
- `r`: restart the selected pane
- `x`: open a capture view of the selected pane
- `z`: open the full fleet cockpit
- `q`: quit the TUI

## Fleet Configuration

For a public setup, think in terms of roles, not personal machine names.

Example shape:

```text
command-center=local
builder-a=dev@builder-a
builder-b=dev@builder-b
edge-a=dev@edge-a
lab-a=dev@lab-a
```

The command-center machine is where you run:

- `Constant`
- the fleet cockpit
- local MLX orchestration
- any human-in-the-loop supervision

The workers are where execution happens.

Public example config:

```bash
mkdir -p ~/.config/constant
cp examples/fleet.example.toml ~/.config/constant/fleet.toml
```

Legacy `~/.config/constant/fleet.json` and `fleet.yaml` are still read for compatibility, but the native format is now TOML.
The shell launchers read both `fleet.toml` and `fleet.json`, so fleet tabs, install/check, and bridge helpers stay aligned during the migration.

## Messaging

Each machine exposes a local bus:

```text
~/.cache/constant/cockpit/<session>/bus
```

Use it from any pane:

```bash
ai-msg.sh send --to codex --message "look at the failing test"
ai-msg.sh send --to vibe --message "explore two alternatives"
ai-msg.sh broadcast --message "new constraints in README"
ai-msg.sh inbox --for claude
```

Bridge between machines from the command center:

```bash
./scripts/ai-bridge.sh send \
  --from-machine command-center \
  --from claude \
  --to-machine lab-a \
  --to codex \
  --message "take over the deep refactor"
```

## Clipboard

Clipboard behavior is split on purpose:

- local macOS sessions can use `pbcopy`
- remote tabs rely on `OSC52`
- the command center keeps ownership of the actual system clipboard

The current runtime relies on `tmux` clipboard forwarding and terminal `OSC52` behavior when you are driving remote sessions from the command center.

## Constant Philosophy

`Constant` is not trying to hide the terminal.

It assumes:

- the shell matters
- sessions matter
- state matters
- local files matter
- naming things matters
- supervision matters

The system should feel like:

- a cockpit
- a score
- a routing table
- a machine that keeps its shape under pressure

Not like a magic trick.

## Public Repo Roadmap

Planned layers for the public `Constant` repo:

- a real demoscene-style TUI
- a central `hexapus` buddy rail
- durable memory with vector + lexical search
- mission summaries and decision graph browsing
- richer repo context and instruction fusion
- cleaner public fleet templates
- screenshots, gifs, and a proper visual identity

## Repo Layout

```text
constant/                         Python orchestration core
src/                              Rust CLI, missions, cockpit, and native TUI
examples/fleet.example.json       public fleet template
examples/fleet.example.toml       public fleet template in the native config format
scripts/Constant                  canonical CLI entrypoint
scripts/constant-deploy.sh        discovery + selection + fleet deployment CLI
scripts/constant-machine.sh       canonical single-machine cockpit entrypoint
scripts/constant-fleet.sh         canonical fleet cockpit entrypoint
scripts/constant-fleet-install.sh canonical fleet installer/checker
scripts/ai-msg.sh                 local agent bus
scripts/ai-bridge.sh              inter-machine bridge
```

## Current Caveats

- the shell runtime still keeps some historical compatibility wrappers
- the public-facing TUI is still evolving
- some setup paths and local assumptions are operator-oriented
- `copilot` is manual-only in the current autonomous flow
- remote `tmux` introspection is best when targets expose `tmux` on a non-interactive shell `PATH`

That is acceptable for now.
The system is being built in the open, from the terminal outward.

## Contributing

If you open this repo and think:

“this is a little unhinged, but technically serious”

then you probably understood the assignment.
