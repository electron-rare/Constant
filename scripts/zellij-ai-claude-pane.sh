#!/usr/bin/env bash
set -euo pipefail

script_source="${BASH_SOURCE[0]:-$0}"
while [[ -L "$script_source" ]]; do
    script_dir="$(cd "$(dirname "$script_source")" && pwd -P)"
    script_source="$(readlink "$script_source")"
    [[ "$script_source" != /* ]] && script_source="$script_dir/$script_source"
done
script_dir="$(cd "$(dirname "$script_source")" && pwd -P)"
source "$script_dir/constant-common.sh"

shell_fallback() {
    local reason="${1:-Claude pane ended.}"
    local shell_bin="${SHELL:-/bin/bash}"
    echo
    echo "$reason"
    echo "Keeping the pane alive in an interactive shell."
    echo "Workspace: $workspace"
    echo
    exec "$shell_bin" -il
}

workspace="${ZELLIJ_AI_WORKSPACE:-$PWD}"
machine_name="${ZELLIJ_AI_MACHINE_NAME:-unknown}"
session_name="${ZELLIJ_AI_SESSION:-$(zellij_ai_default_session)}"
repo_dir="${ZELLIJ_AI_REPO_DIR:-}"
bus_dir="${ZELLIJ_AI_BUS_DIR:-$(zellij_ai_session_state_dir "$session_name")/bus}"

if [[ ! -d "$workspace" ]]; then
    echo "Workspace not found: $workspace" >&2
    exit 1
fi

if [[ -n "${ZELLIJ_AI_CLAUDE_CONFIG_DIR:-}" ]]; then
    export CLAUDE_CONFIG_DIR="$ZELLIJ_AI_CLAUDE_CONFIG_DIR"
fi

export PATH="$(zellij_ai_agent_path)"

if [[ -n "$repo_dir" && -d "$repo_dir/scripts" ]]; then
    export PATH="$repo_dir/scripts:$PATH"
fi

export ZELLIJ_AI_ROLE="claude"
export ZELLIJ_AI_MACHINE_NAME="$machine_name"
export ZELLIJ_AI_SESSION="$session_name"
export ZELLIJ_AI_BUS_DIR="$bus_dir"

cd "$workspace"

echo "Claude pane"
echo "machine: $machine_name"
echo "workspace: $workspace"
echo "bus: $bus_dir"
if [[ -n "$repo_dir" ]]; then
    echo "helper: $repo_dir/scripts/ai-msg.sh"
fi
if [[ -n "${CLAUDE_CONFIG_DIR:-}" ]]; then
    echo "config: $CLAUDE_CONFIG_DIR"
fi
echo

if ! command -v claude >/dev/null 2>&1; then
    shell_fallback "Claude CLI not found on the host."
fi

if ! claude; then
    rc=$?
    shell_fallback "Claude exited with status ${rc}."
fi
