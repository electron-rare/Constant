#!/usr/bin/env bash
set -euo pipefail

script_source="${BASH_SOURCE[0]:-$0}"
while [[ -L "$script_source" ]]; do
    script_dir="$(cd "$(dirname "$script_source")" && pwd -P)"
    script_source="$(readlink "$script_source")"
    [[ "$script_source" != /* ]] && script_source="$script_dir/$script_source"
done
script_dir="$(cd "$(dirname "$script_source")" && pwd -P)"
bootstrap_file="${ZELLIJ_AI_BOOTSTRAP_FILE:-}"
workspace="${ZELLIJ_AI_WORKSPACE:-$PWD}"

if [[ -n "$bootstrap_file" ]]; then
    mkdir -p "$(dirname "$bootstrap_file")"
fi

if [[ -n "$bootstrap_file" && ! -f "$bootstrap_file" ]]; then
    : >"$bootstrap_file"

    printf -v codex_cmd '%q' "$script_dir/zellij-ai-codex-pane.sh"
    printf -v copilot_cmd '%q' "$script_dir/zellij-ai-copilot-pane.sh"
    printf -v vibe_cmd '%q' "$script_dir/zellij-ai-vibe-pane.sh"

    zellij action new-pane -d right -n "${ZELLIJ_AI_CODEX_LABEL:-codex}" --cwd "$workspace"
    zellij action move-focus right
    zellij action write-chars "$codex_cmd"
    zellij action write 10

    zellij action new-pane -d down -n "${ZELLIJ_AI_COPILOT_LABEL:-copilot}" --cwd "$workspace"
    zellij action move-focus down
    zellij action write-chars "$copilot_cmd"
    zellij action write 10

    zellij action new-pane -d down -n "${ZELLIJ_AI_VIBE_LABEL:-vibe}" --cwd "$workspace"
    zellij action move-focus down
    zellij action write-chars "$vibe_cmd"
    zellij action write 10

    zellij action move-focus left
fi

exec "$script_dir/zellij-ai-claude-pane.sh"
