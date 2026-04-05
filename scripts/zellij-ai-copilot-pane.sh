#!/usr/bin/env bash
set -euo pipefail

script_source="${BASH_SOURCE[0]:-$0}"
while [[ -L "$script_source" ]]; do
    script_dir="$(cd "$(dirname "$script_source")" && pwd -P)"
    script_source="$(readlink "$script_source")"
    [[ "$script_source" != /* ]] && script_source="$script_dir/$script_source"
done
script_dir="$(cd "$(dirname "$script_source")" && pwd -P)"
source "$script_dir/zellij-ai-common.sh"

workspace="${ZELLIJ_AI_WORKSPACE:-$PWD}"
machine_name="${ZELLIJ_AI_MACHINE_NAME:-unknown}"
session_name="${ZELLIJ_AI_SESSION:-$(zellij_ai_default_session)}"
repo_dir="${ZELLIJ_AI_REPO_DIR:-$(cd "$script_dir/.." && pwd -P)}"
bus_dir="${ZELLIJ_AI_BUS_DIR:-$(zellij_ai_session_state_dir "$session_name")/bus}"
label="${ZELLIJ_AI_COPILOT_LABEL:-copilot}"

if [[ ! -d "$workspace" ]]; then
    echo "Workspace not found: $workspace" >&2
    exit 1
fi

export PATH="$(zellij_ai_agent_path)"

if [[ -n "$repo_dir" && -d "$repo_dir/scripts" ]]; then
    export PATH="$repo_dir/scripts:$PATH"
fi

if ! command -v copilot >/dev/null 2>&1; then
    echo "GitHub Copilot CLI not found on the host." >&2
    echo "Install it with: npm install -g @github/copilot" >&2
    exit 1
fi

mkdir -p "$bus_dir/messages"

export ZELLIJ_AI_ROLE="$label"
export ZELLIJ_AI_MACHINE_NAME="$machine_name"
export ZELLIJ_AI_SESSION="$session_name"
export ZELLIJ_AI_BUS_DIR="$bus_dir"

cd "$workspace"

echo "Copilot pane"
echo "machine: $machine_name"
echo "workspace: $workspace"
echo "bus: $bus_dir"
if [[ -n "$repo_dir" ]]; then
    echo "helper: $repo_dir/scripts/ai-msg.sh"
fi
echo "login: start copilot and use /login if needed"
echo

copilot
status=$?

if [[ $status -ne 0 ]]; then
    echo
    echo "Copilot exited with status $status."
    echo "If macOS reports 'SecItemCopyMatching failed -50', open copilot manually once and re-run /login."
fi

exit "$status"
