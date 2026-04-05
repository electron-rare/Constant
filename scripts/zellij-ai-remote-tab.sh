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

warn_deprecated() {
    printf 'Warning: %s\n' "$*" >&2
}

usage() {
    local script_name="${CONSTANT_SCRIPT_NAME:-$(basename "$0")}"
    cat <<EOF
Usage: ${script_name} [options]

Open or attach the local Constant machine session for one machine.

Options:
  --label NAME         Display label for logs and tab naming
  --target TARGET      SSH target, or "local" for the current machine
  --session NAME       Remote/local Constant tmux session name
                       default: constant
  --repo-dir DIR       Repository path on the target machine
                       default: $HOME/constant
  --workspace DIR      Workspace path passed to constant-machine.sh
  --codex-image IMAGE  Deprecated, ignored
  --claude-config DIR  Optional Claude config override
  --remote-recreate    Recreate the target machine session before attaching
  -h, --help           Show this help
EOF
}

label=""
target=""
session="$(zellij_ai_default_session)"
repo_dir="$(zellij_ai_default_repo_dir)"
workspace=""
codex_image=""
claude_config_dir=""
remote_recreate=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --label)
            label="$2"
            shift 2
            ;;
        --target)
            target="$2"
            shift 2
            ;;
        --session)
            session="$2"
            shift 2
            ;;
        --repo-dir)
            repo_dir="$2"
            shift 2
            ;;
        --workspace)
            workspace="$2"
            shift 2
            ;;
        --codex-image)
            warn_deprecated "--codex-image is deprecated and ignored; Codex now runs on the host."
            codex_image="$2"
            shift 2
            ;;
        --claude-config)
            claude_config_dir="$2"
            shift 2
            ;;
        --remote-recreate)
            remote_recreate=true
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

if [[ -z "$target" ]]; then
    echo "--target is required" >&2
    usage >&2
    exit 2
fi

if [[ -z "$label" ]]; then
    label="$target"
fi

if zellij_ai_is_local_target "$target"; then
    local_repo_dir="$(zellij_ai_expand_home_path "$repo_dir")"
    local_workspace="$(zellij_ai_expand_home_path "$workspace")"
    local_claude_config_dir="$(zellij_ai_expand_home_path "$claude_config_dir")"
    local_launcher="$local_repo_dir/scripts/constant-machine.sh"
    if [[ ! -x "$local_launcher" ]]; then
        local_launcher="$local_repo_dir/scripts/zellij-ai-triple.sh"
    fi

    if [[ ! -x "$local_launcher" ]]; then
        echo "Local launcher not found for $label: $local_launcher" >&2
        exit 1
    fi

    cmd=("$local_launcher" --session "$session")

    if [[ -n "$workspace" ]]; then
        cmd+=(--workspace "$local_workspace")
    fi
    if [[ -n "$codex_image" ]]; then
        cmd+=(--codex-image "$codex_image")
    fi
    if [[ -n "$claude_config_dir" ]]; then
        cmd+=(--claude-config "$local_claude_config_dir")
    fi
    if $remote_recreate; then
        cmd+=(--recreate)
    fi

    exec env ZELLIJ_AI_MACHINE_NAME="$label" "${cmd[@]}"
fi

printf -v session_q '%q' "$session"
printf -v repo_dir_q '%q' "$repo_dir"
printf -v workspace_q '%q' "$workspace"
printf -v codex_image_q '%q' "$codex_image"
printf -v claude_config_dir_q '%q' "$claude_config_dir"
printf -v label_q '%q' "$label"
remote_recreate_literal=false
if $remote_recreate; then
    remote_recreate_literal=true
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

session=$session_q
repo_dir_input=$repo_dir_q
workspace_input=$workspace_q
codex_image_input=$codex_image_q
claude_config_dir_input=$claude_config_dir_q
remote_recreate=$remote_recreate_literal

repo_dir="\$(expand_home_path "\$repo_dir_input")"
workspace="\$(expand_home_path "\$workspace_input")"
claude_config_dir="\$(expand_home_path "\$claude_config_dir_input")"
launcher="\$repo_dir/scripts/constant-machine.sh"
if [[ ! -x "\$launcher" ]]; then
    launcher="\$repo_dir/scripts/zellij-ai-triple.sh"
fi
if [[ ! -x "\$launcher" ]]; then
    echo "Remote launcher not found: \$launcher" >&2
    exit 1
fi

cmd=("\$launcher" --session "\$session")

if [[ -n "\$workspace_input" ]]; then
    cmd+=(--workspace "\$workspace")
fi
if [[ -n "\$codex_image_input" ]]; then
    cmd+=(--codex-image "\$codex_image_input")
fi
if [[ -n "\$claude_config_dir_input" ]]; then
    cmd+=(--claude-config "\$claude_config_dir")
fi
if \$remote_recreate; then
    cmd+=(--recreate)
fi

export ZELLIJ_AI_MACHINE_NAME=$label_q
export ZELLIJ_AI_FORCE_OSC52=true
exec "\${cmd[@]}"
EOF

exec ssh -tt "$target" "bash -lc $(printf '%q' "$remote_shell")"
