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

usage() {
    local script_name="${CONSTANT_SCRIPT_NAME:-$(basename "$0")}"
    cat <<EOF
Usage: ${script_name} [slot]

slot:
  1     Deprecated alias for the single host-local Codex pane
  2     Deprecated alias for the single host-local Codex pane
EOF
}

warn_deprecated() {
    printf 'Warning: %s\n' "$*" >&2
}

if [[ $# -gt 1 ]]; then
    usage >&2
    exit 2
fi

if [[ $# -eq 1 ]]; then
    case "$1" in
        1)
            warn_deprecated "slot 1 is deprecated; Codex now runs as a single host-local pane."
            ;;
        2)
            warn_deprecated "slot 2 is deprecated; launching the single host-local Codex pane."
            ;;
        -h|--help|help)
            usage
            exit 0
            ;;
        *)
            usage >&2
            exit 2
            ;;
    esac
fi

workspace="${ZELLIJ_AI_WORKSPACE:-$PWD}"
profile_dir="${ZELLIJ_AI_CODEX_HOME:-$(zellij_ai_default_codex_home)}"
label="${ZELLIJ_AI_CODEX_LABEL:-codex}"
main_codex_config="${ZELLIJ_AI_MAIN_CODEX_CONFIG:-$HOME/.codex/config.toml}"
repo_dir="${ZELLIJ_AI_REPO_DIR:-$(cd "$script_dir/.." && pwd -P)}"
session_name="${ZELLIJ_AI_SESSION:-$(zellij_ai_default_session)}"
bus_dir="${ZELLIJ_AI_BUS_DIR:-$(zellij_ai_session_state_dir "$session_name")/bus}"
machine_name="$(zellij_ai_current_machine_name)"
agent_path="$(zellij_ai_agent_path)"

if [[ ! -d "$workspace" ]]; then
    echo "Workspace not found: $workspace" >&2
    exit 1
fi

export PATH="$agent_path"

if ! command -v codex >/dev/null 2>&1; then
    echo "Codex CLI not found on the host." >&2
    echo "Install it with: npm install -g @openai/codex" >&2
    exit 1
fi

profile_dir="$(mkdir -p "$(zellij_ai_expand_home_path "$profile_dir")" && cd "$(zellij_ai_expand_home_path "$profile_dir")" && pwd -P)"
mkdir -p "$bus_dir/messages"

if [[ ! -f "$profile_dir/config.toml" && -f "$main_codex_config" ]]; then
    cp "$main_codex_config" "$profile_dir/config.toml"
fi

if [[ -d "$repo_dir/scripts" ]]; then
    export PATH="$repo_dir/scripts:$PATH"
fi

export CODEX_HOME="$profile_dir"
export ZELLIJ_AI_ROLE="$label"
export ZELLIJ_AI_MACHINE_NAME="$machine_name"
export ZELLIJ_AI_SESSION="$session_name"
export ZELLIJ_AI_BUS_DIR="$bus_dir"

cd "$workspace"

echo "Codex pane"
echo "machine: $machine_name"
echo "workspace: $workspace"
echo "profile: $profile_dir"
echo "bus: $bus_dir"
if [[ -n "$repo_dir" ]]; then
    echo "helper: $repo_dir/scripts/ai-msg.sh"
fi
echo

if [[ ! -f "$CODEX_HOME/auth.json" ]]; then
    echo "No auth.json found for $label."
    echo "Starting: codex login --device-auth"
    echo
    codex login --device-auth
    echo
fi

exec codex --no-alt-screen -C "$workspace"
