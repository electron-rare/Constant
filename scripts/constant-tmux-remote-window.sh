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
Usage: ${script_name} --label LABEL --target TARGET --workspace DIR [options]

Attach a fleet window to the host-local tmux machine session on a local or remote target.

Options:
  --label LABEL            Machine label used for the window title
  --target TARGET          SSH target or local
  --workspace DIR          Workspace path passed to the machine launcher
  --repo-dir DIR           Repo path on the target
                           default: $(zellij_ai_default_repo_dir)
  --session NAME           Machine tmux session name
                           default: $(zellij_ai_default_session)
  --claude-config-dir DIR  Claude config directory on the target
  --recreate               Recreate the target machine session before attach
  -h, --help               Show this help
EOF
}

require_arg() {
    local flag="$1"
    local value="${2:-}"
    if [[ -z "$value" ]]; then
        echo "Missing value for ${flag}" >&2
        exit 2
    fi
}

label=""
target=""
workspace=""
repo_dir="$(zellij_ai_default_repo_dir)"
session="$(zellij_ai_default_session)"
claude_config_dir=""
recreate=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --label)
            require_arg "$1" "${2:-}"
            label="$2"
            shift 2
            ;;
        --target)
            require_arg "$1" "${2:-}"
            target="$2"
            shift 2
            ;;
        --workspace)
            require_arg "$1" "${2:-}"
            workspace="$2"
            shift 2
            ;;
        --repo-dir)
            require_arg "$1" "${2:-}"
            repo_dir="$2"
            shift 2
            ;;
        --session)
            require_arg "$1" "${2:-}"
            session="$2"
            shift 2
            ;;
        --claude-config-dir)
            require_arg "$1" "${2:-}"
            claude_config_dir="$2"
            shift 2
            ;;
        --recreate)
            recreate=true
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "Unknown option: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

[[ -n "$label" ]] || { usage >&2; exit 2; }
[[ -n "$target" ]] || { usage >&2; exit 2; }
[[ -n "$workspace" ]] || { usage >&2; exit 2; }

if zellij_ai_is_local_target "$target"; then
    local_repo_dir="$(zellij_ai_expand_home_path "$repo_dir")"
    local_workspace="$(zellij_ai_expand_home_path "$workspace")"
    if [[ ! -d "$local_workspace" ]]; then
        local_workspace="$local_repo_dir"
    fi
    launcher="$local_repo_dir/scripts/constant-machine.sh"
    args=(
        --workspace "$local_workspace"
        --session "$session"
        --repo-dir "$local_repo_dir"
    )
    [[ -x "$launcher" ]] || launcher="$local_repo_dir/scripts/constant-tmux-machine.sh"
    if [[ -n "$claude_config_dir" ]]; then
        args+=(--claude-config-dir "$claude_config_dir")
    fi
    if $recreate; then
        args+=(--recreate)
    fi
    exec env ZELLIJ_AI_MACHINE_NAME="$label" "$launcher" "${args[@]}"
fi

printf -v repo_dir_q '%q' "$repo_dir"
printf -v workspace_q '%q' "$workspace"
printf -v session_q '%q' "$session"
printf -v claude_config_q '%q' "$claude_config_dir"
printf -v label_q '%q' "$label"
recreate_literal=false
if $recreate; then
    recreate_literal=true
fi

read -r -d '' remote_shell <<EOF || true
set -euo pipefail

expand_home_path() {
    case "\$1" in
        '\$HOME'|'\$HOME/'*)
            printf '%s\n' "\${HOME}\${1#\\\$HOME}"
            ;;
        "~")
            printf '%s\n' "\$HOME"
            ;;
        "~/"*)
            printf '%s\n' "\$HOME/\${1#~/}"
            ;;
        *)
            printf '%s\n' "\$1"
            ;;
    esac
}

repo_dir_input=$repo_dir_q
workspace_input=$workspace_q
session_name=$session_q
claude_config_dir_input=$claude_config_q
recreate=$recreate_literal

repo_dir="\$(expand_home_path "\$repo_dir_input")"
workspace="\$(expand_home_path "\$workspace_input")"
claude_config_dir="\$(expand_home_path "\$claude_config_dir_input")"
if [[ ! -d "\$workspace" ]]; then
    workspace="\$repo_dir"
fi
launcher="\$repo_dir/scripts/constant-machine.sh"
if [[ ! -x "\$launcher" ]]; then
    launcher="\$repo_dir/scripts/constant-tmux-machine.sh"
fi

args=(--workspace "\$workspace" --session "\$session_name" --repo-dir "\$repo_dir")
if [[ -n "\$claude_config_dir_input" ]]; then
    args+=(--claude-config-dir "\$claude_config_dir")
fi
if \$recreate; then
    args+=(--recreate)
fi

exec env ZELLIJ_AI_MACHINE_NAME=$label_q ZELLIJ_AI_FORCE_OSC52=true "\$launcher" "\${args[@]}"
EOF

exec ssh -tt "$target" "bash -lc $(printf '%q' "$remote_shell")"
