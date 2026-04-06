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
Usage: ${script_name} [options]

Create or control the host-local tmux cockpit for one machine.

Options:
  --workspace DIR        Workspace for all 4 panes
                         default: current directory
  --session NAME         Machine tmux session name
                         default: $(zellij_ai_default_session)
  --repo-dir DIR         Repo containing Constant scripts
                         default: $(zellij_ai_default_repo_dir)
  --codex-home DIR       Codex profile directory
                         default: $(zellij_ai_default_codex_home)
  --claude-config-dir D  Claude config directory passed to the Claude pane
  --recreate             Kill and recreate the machine session
  --ensure-only          Ensure the session exists, then exit
  --focus-pane ROLE      Focus one pane: claude|codex|copilot|vibe
  --capture-pane ROLE    Print pane capture for the selected role
  --send-pane ROLE       Send --command text to the selected pane
  --restart-pane ROLE    Respawn the selected pane with its launcher command
  --command TEXT         Text to send with --send-pane
  --lines N              Lines to print with --capture-pane
                         default: 120
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

tmux_target_window() {
    printf '%s:%s\n' "$session" "$machine_name"
}

role_pane_id() {
    local role="$1"
    tmux list-panes -t "$(tmux_target_window)" -F '#{pane_id}	#{@constant_role}	#{pane_title}' 2>/dev/null | awk -F '\t' -v role="$role" '$2 == role || $3 == role {print $1; exit}'
}

ensure_workspace() {
    if [[ ! -d "$workspace" ]]; then
        echo "Workspace not found: $workspace" >&2
        exit 1
    fi
}

pane_command() {
    constant_tmux_role_command "$1" "$script_dir" "$workspace" "$repo_dir" "$session" "$bus_dir" "$machine_name" "$codex_home" "$claude_config_dir"
}

set_pane_meta() {
    local pane_id="$1"
    local role="$2"
    local command_string="${3:-}"
    local reset_state="${4:-0}"
    if [[ -n "$command_string" ]]; then
        constant_tmux_set_managed_pane "$pane_id" "$role" "$command_string" "$reset_state"
        return
    fi
    tmux select-pane -t "$pane_id" -T "$role" >/dev/null 2>&1 || true
    tmux set-option -p -t "$pane_id" @constant_role "$role" >/dev/null 2>&1 || true
}

create_session() {
    local target_window claude_cmd codex_cmd copilot_cmd vibe_cmd
    local claude_pane codex_pane copilot_pane vibe_pane

    target_window="$(tmux_target_window)"
    claude_cmd="$(pane_command claude)"
    codex_cmd="$(pane_command codex)"
    copilot_cmd="$(pane_command copilot)"
    vibe_cmd="$(pane_command vibe)"

    tmux new-session -d -s "$session" -n "$machine_name" "$claude_cmd"
    constant_tmux_configure_autorestart_hook "$session"
    constant_tmux_configure_status_chrome "$session" "$machine_name" "colour81"
    constant_tmux_configure_chat_dock "$session" "$repo_dir" "$workspace" "$machine_name" "$machine_name"
    tmux set-window-option -t "$target_window" -g pane-base-index 0 >/dev/null 2>&1 || true

    claude_pane="$(tmux list-panes -t "$target_window" -F '#{pane_id}' | head -n 1)"
    codex_pane="$(tmux split-window -h -P -F '#{pane_id}' -t "$claude_pane" "$codex_cmd")"
    copilot_pane="$(tmux split-window -v -P -F '#{pane_id}' -t "$codex_pane" "$copilot_cmd")"
    vibe_pane="$(tmux split-window -v -P -F '#{pane_id}' -t "$copilot_pane" "$vibe_cmd")"

    tmux select-layout -t "$target_window" main-vertical >/dev/null
    tmux resize-pane -t "$claude_pane" -x 120 >/dev/null 2>&1 || true
    set_pane_meta "$claude_pane" claude "$claude_cmd" 1
    set_pane_meta "$codex_pane" codex "$codex_cmd" 1
    set_pane_meta "$copilot_pane" copilot "$copilot_cmd" 1
    set_pane_meta "$vibe_pane" vibe "$vibe_cmd" 1
    tmux select-pane -t "$claude_pane" >/dev/null
}

ensure_session() {
    local role pane_id command_string
    mkdir -p "$state_dir" "$bus_dir/messages"

    if $recreate && constant_tmux_session_exists "$session"; then
        tmux kill-session -t "$session" >/dev/null 2>&1 || true
    fi

    if ! constant_tmux_session_exists "$session"; then
        create_session
        return 0
    fi

    constant_tmux_configure_autorestart_hook "$session"
    constant_tmux_configure_status_chrome "$session" "$machine_name" "colour81"
    constant_tmux_configure_chat_dock "$session" "$repo_dir" "$workspace" "$machine_name" "$machine_name"

    if ! constant_tmux_window_exists "$session" "$machine_name"; then
        recreate=true
        tmux kill-session -t "$session" >/dev/null 2>&1 || true
        create_session
        return 0
    fi

    for role in claude codex copilot vibe; do
        pane_id="$(role_pane_id "$role")"
        if [[ -z "$pane_id" ]]; then
            continue
        fi
        command_string="$(pane_command "$role")"
        set_pane_meta "$pane_id" "$role" "$command_string"
    done
}

focus_role() {
    local role="$1"
    local pane_id

    ensure_session
    pane_id="$(role_pane_id "$role")"
    if [[ -z "$pane_id" ]]; then
        echo "No pane found for role: $role" >&2
        exit 1
    fi

    tmux select-window -t "$(tmux_target_window)" >/dev/null
    tmux select-pane -t "$pane_id" >/dev/null
}

restart_role() {
    local role="$1"
    local pane_id command_string

    ensure_session
    pane_id="$(role_pane_id "$role")"
    if [[ -z "$pane_id" ]]; then
        echo "No pane found for role: $role" >&2
        exit 1
    fi

    command_string="$(pane_command "$role")"
    tmux respawn-pane -k -t "$pane_id" "$command_string"
    set_pane_meta "$pane_id" "$role" "$command_string" 1
    tmux select-layout -t "$(tmux_target_window)" main-vertical >/dev/null
}

capture_role() {
    local role="$1"
    local pane_id

    ensure_session
    pane_id="$(role_pane_id "$role")"
    if [[ -z "$pane_id" ]]; then
        echo "No pane found for role: $role" >&2
        exit 1
    fi

    tmux capture-pane -p -S "-${lines}" -t "$pane_id"
}

send_role() {
    local role="$1"
    local pane_id

    require_arg --command "$send_command"
    ensure_session
    pane_id="$(role_pane_id "$role")"
    if [[ -z "$pane_id" ]]; then
        echo "No pane found for role: $role" >&2
        exit 1
    fi

    tmux send-keys -t "$pane_id" -l "$send_command"
    tmux send-keys -t "$pane_id" Enter
}

session="$(zellij_ai_default_session)"
workspace="$PWD"
repo_dir="$(cd "$script_dir/.." && pwd -P)"
codex_home="$(zellij_ai_expand_home_path "$(zellij_ai_default_codex_home)")"
claude_config_dir=""
machine_name="${ZELLIJ_AI_MACHINE_NAME:-$(zellij_ai_local_machine_label)}"
recreate=false
ensure_only=false
focus_pane=""
capture_pane=""
restart_pane=""
send_pane=""
send_command=""
lines=120

while [[ $# -gt 0 ]]; do
    case "$1" in
        --workspace)
            require_arg "$1" "${2:-}"
            workspace="$(zellij_ai_expand_home_path "$2")"
            shift 2
            ;;
        --session)
            require_arg "$1" "${2:-}"
            session="$2"
            shift 2
            ;;
        --repo-dir)
            require_arg "$1" "${2:-}"
            repo_dir="$(zellij_ai_expand_home_path "$2")"
            shift 2
            ;;
        --codex-home)
            require_arg "$1" "${2:-}"
            codex_home="$(zellij_ai_expand_home_path "$2")"
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
        --ensure-only)
            ensure_only=true
            shift
            ;;
        --focus-pane)
            require_arg "$1" "${2:-}"
            focus_pane="$2"
            shift 2
            ;;
        --capture-pane)
            require_arg "$1" "${2:-}"
            capture_pane="$2"
            shift 2
            ;;
        --send-pane)
            require_arg "$1" "${2:-}"
            send_pane="$2"
            shift 2
            ;;
        --restart-pane)
            require_arg "$1" "${2:-}"
            restart_pane="$2"
            shift 2
            ;;
        --command)
            require_arg "$1" "${2:-}"
            send_command="$2"
            shift 2
            ;;
        --lines)
            require_arg "$1" "${2:-}"
            lines="$2"
            shift 2
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

constant_tmux_require
ensure_workspace
state_dir="$(zellij_ai_session_state_dir "$session")"
bus_dir="$state_dir/bus"

if [[ -n "$focus_pane" ]]; then
    focus_role "$focus_pane"
    exit 0
fi

if [[ -n "$capture_pane" ]]; then
    capture_role "$capture_pane"
    exit 0
fi

if [[ -n "$send_pane" ]]; then
    send_role "$send_pane"
    exit 0
fi

if [[ -n "$restart_pane" ]]; then
    restart_role "$restart_pane"
    exit 0
fi

ensure_session

if $ensure_only; then
    exit 0
fi

if [[ -n "${TMUX:-}" ]]; then
    exec tmux switch-client -t "$session"
else
    exec tmux attach-session -t "$session"
fi
