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

printf -v remote_inner 'repo_dir=%q; workspace=%q; session_name=%q; repo_dir="${repo_dir#\\}"; workspace="${workspace#\\}"; case "$repo_dir" in \$HOME|\$HOME/*) repo_dir="${HOME}${repo_dir#\$HOME}" ;; "~") repo_dir="$HOME" ;; ~/*) repo_dir="$HOME/${repo_dir#~/}" ;; esac; case "$workspace" in \$HOME|\$HOME/*) workspace="${HOME}${workspace#\$HOME}" ;; "~") workspace="$HOME" ;; ~/*) workspace="$HOME/${workspace#~/}" ;; esac; launcher="$repo_dir/scripts/constant-machine.sh"; if [[ ! -x "$launcher" ]]; then launcher="$repo_dir/scripts/constant-tmux-machine.sh"; fi; args=(--workspace "$workspace" --session "$session_name" --repo-dir "$repo_dir");' \
    "$repo_dir" \
    "$workspace" \
    "$session"

if [[ -n "$claude_config_dir" ]]; then
    printf -v remote_inner '%s args+=(--claude-config-dir %q);' "$remote_inner" "$claude_config_dir"
fi

if $recreate; then
    printf -v remote_inner '%s args+=(--recreate);' "$remote_inner"
fi

printf -v remote_inner '%s exec env ZELLIJ_AI_MACHINE_NAME=%q ZELLIJ_AI_FORCE_OSC52=true "$launcher" "${args[@]}"' "$remote_inner" "$label"
exec ssh -tt "$target" bash -lc "$remote_inner"
