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

- a 4-pane Zellij machine cockpit
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

- a 4-pane host-local Zellij session per machine
- a fleet cockpit with one tab per machine
- `Constant`, the orchestration CLI on top of the cockpit
- a terminal TUI with a central `hexapus` buddy rail
- local mission planning and verification
- host-local execution for `claude`, `codex`, and `vibe`
- `copilot` as a manual lane
- local message bus + cross-machine bridge
- MLX-ready model plumbing for a small local orchestrator stack on macOS
- workspace-first durable memory with lexical + local vector search

The historical repo name may still say `zellij-ai-triple`.
The product name is now `Constant`.

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

From your command center machine, you open one Zellij tab per machine.
Each tab attaches to the machine’s own local session.

That gives you:

- one place to supervise the whole fleet
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

- `zellij`
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

By default, the machine launcher creates a fresh temporary Zellij config directory for each new session.
That is deliberate: it isolates `Constant` from an existing user Zellij config, custom layouts, plugins,
and stale resurrected state. If you want to pin a specific Zellij config dir, use:

```bash
./scripts/constant-machine.sh --zellij-config-dir /path/to/zellij-config
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

In interactive mode, `Constant` with no arguments now opens the TUI by default.

### Discover and deploy a fleet

`Constant` now ships with a public discovery/deployment CLI that:

- scans `~/.ssh/config`
- scans `~/.ssh/known_hosts`
- scans local `arp -a` neighbors
- validates candidates with a short SSH probe
- lets you select targets interactively
- asks for SSH user and machine labels
- writes `~/.config/constant/fleet.json`
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

If a `fleet.json` contains `repo_dir`, the shell launchers and fleet installer will reuse it automatically.

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

`Constant tui` gives you:

- a mission deck
- a mission board
- a `hexapus` buddy rail
- a bottom timeline mixed with memory echoes

Useful keys:

- `j` / `k`: move between missions
- `e`: rebuild memory for the current workspace
- `s`: summarize the selected mission into durable memory
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
cp examples/fleet.example.json ~/.config/constant/fleet.json
```

Legacy `~/.config/constant/fleet.yaml` is still read for compatibility, but the public-facing format is now JSON.
The shell launchers also read `fleet.json` now, so fleet tabs, install/check, and bridge helpers stay aligned with the same config.

## Messaging

Each machine exposes a local bus:

```text
~/.cache/constant/zellij/<session>/bus
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

This matters.

If a remote pane copies text, it should land in the operator machine clipboard, not in some random remote shell session.

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
examples/fleet.example.json       public fleet template
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

That is acceptable for now.
The system is being built in the open, from the terminal outward.

## Contributing

If you open this repo and think:

“this is a little unhinged, but technically serious”

then you probably understood the assignment.
