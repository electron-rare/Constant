# Zellij AI Triple

Three-pane Zellij workspace:

- left pane: `claude` on the host
- top-right pane: `codex` in Docker with profile 1
- bottom-right pane: `codex` in Docker with profile 2

Each Codex pane gets its own `CODEX_HOME`, so each one can log into a different ChatGPT/OpenAI account.

## Requirements

- `zellij`
- `claude`
- `codex`
- `docker`
- a local Docker image with `bash` and `git`

Default Codex container image:

- `codercom/code-server:latest`

## Usage

```bash
./scripts/zellij-ai-triple.sh
```

Useful flags:

```bash
./scripts/zellij-ai-triple.sh \
  --workspace /path/to/project \
  --session ai-triple \
  --codex-image codercom/code-server:latest \
  --codex1-home "$HOME/.codex-profiles/codex-1" \
  --codex2-home "$HOME/.codex-profiles/codex-2" \
  --recreate
```

By default, the launcher creates a fresh temporary Zellij config directory for each new session.
This is intentional: it avoids interference from an existing user Zellij config, custom default
layouts, plugins, or resurrected session state. If you really want to use a specific Zellij config
directory, pass:

```bash
./scripts/zellij-ai-triple.sh --zellij-config-dir /path/to/zellij-config
```

On first launch of each Codex pane, if `auth.json` is missing in its profile, the pane runs:

```bash
codex login --device-auth
```

Use a different account in each pane if you want true account separation.

## Security

This repository is intended to stay secret-free:

- no `auth.json`
- no `.env`
- no API keys
- no profile directories

The provided `.gitignore` excludes common secret-bearing files and local runtime state.
