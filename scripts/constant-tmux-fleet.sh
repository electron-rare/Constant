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
Usage: ${script_name} --workspace DIR [options]

Open the Constant tmux fleet cockpit with one central window and one window per machine.

Options:
  --workspace DIR        Shared workspace path
  --session NAME         Machine tmux session name
                         default: $(zellij_ai_default_session)
  --local-session NAME   Fleet tmux session name on this machine
                         default: $(zellij_ai_default_local_session)
  --repo-dir DIR         Constant repo path on each machine
                         default: $(zellij_ai_default_repo_dir)
  --machine SPEC         Machine definition or label to include
  --claude-config-dir D  Claude config directory for the Claude pane
  --recreate             Recreate the local fleet session before launch
  --remote-recreate      Recreate each machine tmux session before attach
  --attach-only          Attach to an existing local fleet session
  --ensure-only          Create/update the fleet session, then exit
  -h, --help             Show this help
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

window_pane_id() {
    local session_name="$1"
    local window_name="$2"
    tmux list-panes -t "${session_name}:${window_name}" -F '#{pane_id}' 2>/dev/null | head -n 1
}

mark_window_managed() {
    local session_name="$1"
    local window_name="$2"
    local role_label="$3"
    local command_string="$4"
    local pane_id

    pane_id="$(window_pane_id "$session_name" "$window_name")"
    if [[ -z "$pane_id" ]]; then
        return 1
    fi
    constant_tmux_set_managed_pane "$pane_id" "$role_label" "$command_string"
}

build_constant_window_command() {
    local cmd=(
        env
        "PATH=$(zellij_ai_agent_path)"
        "$repo_dir/scripts/Constant"
        tui
        --workspace "$workspace"
        --local-session "$local_session"
        --session "$remote_session"
    )
    printf '%q ' "${cmd[@]}"
}

build_remote_window_command() {
    local label="$1"
    local target="$2"
    local remote_script cmd

    remote_script="$repo_dir/scripts/constant-tmux-remote-window.sh"
    cmd=(
        "$remote_script"
        --label "$label"
        --target "$target"
        --workspace "$workspace_spec"
        --repo-dir "$repo_dir_spec"
        --session "$remote_session"
    )
    if [[ -n "$claude_config_dir" ]]; then
        cmd+=(--claude-config-dir "$claude_config_dir")
    fi
    if $remote_recreate; then
        cmd+=(--recreate)
    fi

    printf '%q ' "${cmd[@]}"
}

link_local_window() {
    local label="$1"
    local launcher="$repo_dir/scripts/constant-machine.sh"
    local args=(
        --workspace "$workspace"
        --session "$remote_session"
        --repo-dir "$repo_dir"
        --ensure-only
    )

    if [[ ! -x "$launcher" ]]; then
        launcher="$repo_dir/scripts/constant-tmux-machine.sh"
    fi
    if [[ -n "$claude_config_dir" ]]; then
        args+=(--claude-config-dir "$claude_config_dir")
    fi
    if $remote_recreate; then
        args+=(--recreate)
    fi

    ZELLIJ_AI_MACHINE_NAME="$label" "$launcher" "${args[@]}"
    if ! constant_tmux_window_exists "$local_session" "$label"; then
        tmux link-window -d -s "${remote_session}:${label}" -t "${local_session}:"
    fi
}

configure_fleet_session() {
    local session_name="$1"
    tmux rename-window -t "${session_name}:0" Constant >/dev/null 2>&1 || true
    constant_tmux_configure_autorestart_hook "$session_name"
    constant_tmux_configure_status_chrome "$session_name" "Constant" "green"
    constant_tmux_configure_chat_dock "$session_name" "$repo_dir" "$workspace" "fleet" ""
}

attach_fleet_session() {
    local session_name="$1"
    tmux select-window -t "${session_name}:Constant" >/dev/null 2>&1 || true
    if [[ -n "${TMUX:-}" ]]; then
        exec tmux switch-client -t "$session_name"
    fi
    exec tmux attach-session -t "$session_name"
}

window_pane_dead() {
    local session_name="$1"
    local window_name="$2"
    local dead
    dead="$(tmux display-message -p -t "${session_name}:${window_name}" '#{pane_dead}' 2>/dev/null || true)"
    [[ "$dead" == "1" ]]
}

ensure_constant_window() {
    local command_string
    command_string="$(build_constant_window_command)"
    if constant_tmux_window_exists "$local_session" "Constant"; then
        if window_pane_dead "$local_session" "Constant"; then
            tmux respawn-pane -k -t "${local_session}:Constant" "$command_string"
        fi
        mark_window_managed "$local_session" "Constant" "constant" "$command_string" >/dev/null 2>&1 || true
    else
        tmux new-window -d -t "${local_session}:" -n Constant "$command_string"
        mark_window_managed "$local_session" "Constant" "constant" "$command_string" >/dev/null 2>&1 || true
    fi
}

ensure_remote_window() {
    local label="$1"
    local target="$2"
    local command_string
    command_string="$(build_remote_window_command "$label" "$target")"
    if constant_tmux_window_exists "$local_session" "$label"; then
        if window_pane_dead "$local_session" "$label"; then
            tmux respawn-pane -k -t "${local_session}:${label}" "$command_string"
        fi
        mark_window_managed "$local_session" "$label" "$label" "$command_string" >/dev/null 2>&1 || true
    else
        tmux new-window -d -t "${local_session}:" -n "$label" "$command_string"
        mark_window_managed "$local_session" "$label" "$label" "$command_string" >/dev/null 2>&1 || true
    fi
}

ensure_fleet_windows() {
    local machine_spec parsed_spec label target

    ensure_constant_window
    for machine_spec in "${machines[@]}"; do
        parsed_spec="$(zellij_ai_parse_machine_spec "$machine_spec")"
        IFS=$'\t' read -r label target <<<"$parsed_spec"

        if [[ "$label" == "Constant" ]]; then
            continue
        fi

        if zellij_ai_is_local_target "$target"; then
            link_local_window "$label"
        else
            ensure_remote_window "$label" "$target"
        fi
    done
}

constant_tmux_require
zellij_ai_require_command ssh

workspace=""
workspace_spec=""
remote_session="$(zellij_ai_default_session)"
local_session="$(zellij_ai_default_local_session)"
repo_dir="$(zellij_ai_expand_home_path "$(zellij_ai_default_repo_dir)")"
repo_dir_spec="$(zellij_ai_default_repo_dir)"
claude_config_dir=""
recreate=false
remote_recreate=false
attach_only=false
ensure_only=false
machines=()

while [[ $# -gt 0 ]]; do
    case "$1" in
        --workspace)
            require_arg "$1" "${2:-}"
            workspace_spec="$2"
            workspace="$(zellij_ai_expand_home_path "$2")"
            shift 2
            ;;
        --session)
            require_arg "$1" "${2:-}"
            remote_session="$2"
            shift 2
            ;;
        --local-session)
            require_arg "$1" "${2:-}"
            local_session="$2"
            shift 2
            ;;
        --repo-dir)
            require_arg "$1" "${2:-}"
            repo_dir_spec="$2"
            repo_dir="$(zellij_ai_expand_home_path "$2")"
            shift 2
            ;;
        --machine)
            require_arg "$1" "${2:-}"
            machines+=("$2")
            shift 2
            ;;
        --claude-config-dir)
            require_arg "$1" "${2:-}"
            claude_config_dir="$(zellij_ai_expand_home_path "$2")"
            shift 2
            ;;
        --recreate)
            recreate=true
            shift
            ;;
        --remote-recreate)
            remote_recreate=true
            shift
            ;;
        --attach-only)
            attach_only=true
            shift
            ;;
        --ensure-only)
            ensure_only=true
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

if $attach_only; then
    attach_fleet_session "$local_session"
fi

if [[ -z "$workspace" ]]; then
    usage >&2
    exit 2
fi
if [[ ! -d "$workspace" ]]; then
    echo "Workspace not found: $workspace" >&2
    exit 1
fi

if [[ ${#machines[@]} -eq 0 ]]; then
    mapfile -t machines < <(zellij_ai_default_machine_specs)
fi

if $recreate && constant_tmux_session_exists "$local_session"; then
    tmux kill-session -t "$local_session" >/dev/null 2>&1 || true
fi

if constant_tmux_session_exists "$local_session"; then
    configure_fleet_session "$local_session"
    ensure_fleet_windows
    tmux select-window -t "${local_session}:Constant" >/dev/null 2>&1 || true
    if $ensure_only; then
        exit 0
    fi
    attach_fleet_session "$local_session"
fi

mkdir -p "$(zellij_ai_fleet_state_dir "$local_session")"
tmux new-session -d -s "$local_session" -n Constant "$(build_constant_window_command)"
configure_fleet_session "$local_session"

for machine_spec in "${machines[@]}"; do
    parsed_spec="$(zellij_ai_parse_machine_spec "$machine_spec")"
    IFS=$'\t' read -r label target <<<"$parsed_spec"

    if [[ "$label" == "Constant" ]]; then
        continue
    fi

    if zellij_ai_is_local_target "$target"; then
        link_local_window "$label"
    else
        tmux new-window -d -t "${local_session}:" -n "$label" "$(build_remote_window_command "$label" "$target")"
    fi
done

tmux select-window -t "${local_session}:Constant" >/dev/null 2>&1 || true

if $ensure_only; then
    exit 0
fi

attach_fleet_session "$local_session"
