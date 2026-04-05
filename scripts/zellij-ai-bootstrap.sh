#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
bootstrap_file="${ZELLIJ_AI_BOOTSTRAP_FILE:-}"
workspace="${ZELLIJ_AI_WORKSPACE:-$PWD}"

if [[ -n "$bootstrap_file" ]]; then
    mkdir -p "$(dirname "$bootstrap_file")"
fi

if [[ -n "$bootstrap_file" && ! -f "$bootstrap_file" ]]; then
    : >"$bootstrap_file"

    printf -v codex1_cmd '%q %q' "$script_dir/zellij-ai-codex-pane.sh" "1"
    printf -v codex2_cmd '%q %q' "$script_dir/zellij-ai-codex-pane.sh" "2"

    zellij action new-pane -d right -n "${ZELLIJ_AI_CODEX1_LABEL:-codex-1}" --cwd "$workspace"
    zellij action move-focus right
    zellij action write-chars "$codex1_cmd"
    zellij action write 10

    zellij action new-pane -d down -n "${ZELLIJ_AI_CODEX2_LABEL:-codex-2}" --cwd "$workspace"
    zellij action move-focus down
    zellij action write-chars "$codex2_cmd"
    zellij action write 10

    zellij action move-focus left
fi

exec "$script_dir/zellij-ai-claude-pane.sh"
