# Contributing to Constant

`Constant` is a terminal-first orchestration project.
Contributions should keep that bias visible in the code and in the product shape.

## What We Care About

- local-first workflows before cloud assumptions
- real shells, real sessions, real files
- clean operator ergonomics
- explicit routing and supervision
- durable context instead of prompt spaghetti
- strong terminal aesthetics without gimmick-only UX

## Good Contributions

- make the cockpit more reliable
- improve mission routing or verification
- improve durable memory and repo context
- sharpen the TUI and the `hexapus` buddy rail
- simplify setup without hiding the system
- improve docs with concrete operator value

## Ground Rules

- keep the CLI useful in non-interactive environments
- prefer robust shell and Python over magic abstractions
- preserve local observability
- do not hardcode personal machine names, users, IPs, or paths
- treat clipboard, SSH, and session behavior as product features, not side details

## Style

- favor direct, composable commands
- prefer explicit state over hidden side effects
- keep defaults safe and public-repo friendly
- avoid introducing heavyweight infrastructure unless it clearly pays for itself
- maintain the visual identity: terminal-native, slightly demoscene, still readable

## Before Opening a PR

Run at least the checks relevant to your change.

Examples:

```bash
python3 -m py_compile constant/*.py
bash -n scripts/*.sh
./scripts/Constant doctor
```

If your change affects routing, fleet behavior, or memory shape, include a short explanation of:

- what changed
- how you tested it
- what assumptions still remain

## Design Notes

If you add a new capability, ask:

1. does this help an operator supervise a real machine or fleet?
2. does it make routing or context more explicit?
3. does it still degrade cleanly when the environment is constrained?

If the answer is no, it probably does not belong here yet.
