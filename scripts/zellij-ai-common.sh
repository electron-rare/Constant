#!/usr/bin/env bash

zellij_ai_config_root() {
    printf '%s\n' "${CONSTANT_CONFIG_ROOT:-$HOME/.config/constant}"
}

zellij_ai_fleet_config_candidates() {
    if [[ -n "${CONSTANT_FLEET_CONFIG:-}" ]]; then
        printf '%s\n' "$CONSTANT_FLEET_CONFIG"
    fi
    printf '%s\n' "$(zellij_ai_config_root)/fleet.toml"
    printf '%s\n' "$(zellij_ai_config_root)/fleet.json"
    printf '%s\n' "$(zellij_ai_config_root)/fleet.yaml"
}

zellij_ai_fleet_config_path() {
    local candidate
    while IFS= read -r candidate; do
        [[ -z "$candidate" ]] && continue
        if [[ -f "$candidate" ]]; then
            printf '%s\n' "$candidate"
            return 0
        fi
    done < <(zellij_ai_fleet_config_candidates)
    return 1
}

zellij_ai_fleet_config_query() {
    local expr="$1"
    local config_path
    local common_source common_dir wrapper

    config_path="$(zellij_ai_fleet_config_path 2>/dev/null || true)"
    [[ -n "$config_path" ]] || return 1
    common_source="${BASH_SOURCE[0]:-$0}"
    common_dir="$(cd "$(dirname "$common_source")" && pwd -P)"
    wrapper="$common_dir/Constant"
    [[ -x "$wrapper" ]] || return 1

    "$wrapper" fleet config-get "$expr"
}

zellij_ai_default_session() {
    printf '%s\n' "constant"
}

zellij_ai_default_local_session() {
    printf '%s\n' "constant-fleet"
}

zellij_ai_default_repo_dir() {
    local configured
    configured="$(zellij_ai_fleet_config_query repo_dir 2>/dev/null || true)"
    if [[ -n "$configured" ]]; then
        printf '%s\n' "$configured"
    else
        printf '%s\n' '$HOME/constant'
    fi
}

zellij_ai_default_codex_home() {
    printf '%s\n' '$HOME/.codex-profiles/codex'
}

zellij_ai_default_codex_image() {
    printf '%s\n' "codercom/code-server:latest"
}

zellij_ai_runtime_root() {
    printf '%s\n' "${ZELLIJ_AI_RUNTIME_ROOT:-$HOME/.cache/constant/cockpit}"
}

zellij_ai_session_state_dir() {
    printf '%s\n' "$(zellij_ai_runtime_root)/$1"
}

zellij_ai_fleet_state_dir() {
    printf '%s\n' "$(zellij_ai_runtime_root)/$1-fleet"
}

zellij_ai_bridge_cache_dir() {
    printf '%s\n' "$(zellij_ai_runtime_root)/bridge-cache"
}

zellij_ai_default_machine_specs() {
    local configured
    configured="$(zellij_ai_fleet_config_query machine_specs 2>/dev/null || true)"
    if [[ -n "$configured" ]]; then
        printf '%s\n' "$configured"
        return 0
    fi

    cat <<'EOF'
command-center=local
builder-a=dev@builder-a
builder-b=dev@builder-b
edge-a=dev@edge-a
lab-a=dev@lab-a
EOF
}

zellij_ai_strip_ansi() {
    sed -E 's/\x1b\[[0-9;]*m//g'
}

zellij_ai_session_exists() {
    tmux has-session -t "$1" >/dev/null 2>&1
}

zellij_ai_kdl_escape() {
    printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

zellij_ai_expand_home_path() {
    case "$1" in
        '$HOME'|'$HOME/'*)
            printf '%s\n' "${HOME}${1#\$HOME}"
            ;;
        "~")
            printf '%s\n' "$HOME"
            ;;
        "~/"*)
            printf '%s\n' "$HOME/${1#~/}"
            ;;
        *)
            printf '%s\n' "$1"
            ;;
    esac
}

zellij_ai_expand_home_path_for_home() {
    local value="$1"
    local target_home="$2"

    case "$value" in
        '$HOME'|'$HOME/'*)
            printf '%s\n' "${target_home}${value#\$HOME}"
            ;;
        "~")
            printf '%s\n' "$target_home"
            ;;
        "~/"*)
            printf '%s\n' "$target_home/${value#~/}"
            ;;
        *)
            printf '%s\n' "$value"
            ;;
    esac
}

zellij_ai_is_local_target() {
    local value="$1"
    local current_full current_short

    current_full="$(hostname || true)"
    current_short="$(hostname -s 2>/dev/null || printf '%s' "$current_full")"

    case "$value" in
        local|localhost|127.0.0.1|::1|"${current_full}"|"${current_short}")
            return 0
            ;;
        *)
            return 1
            ;;
    esac
}

zellij_ai_parse_machine_spec() {
    local spec="$1"
    local label target

    if [[ "$spec" == *"="* ]]; then
        label="${spec%%=*}"
        target="${spec#*=}"
    else
        label="$spec"
        target="$spec"
    fi

    if [[ -z "$label" || -z "$target" ]]; then
        return 1
    fi

    printf '%s\t%s\n' "$label" "$target"
}

zellij_ai_resolve_machine_spec() {
    local needle="$1"
    local line label target

    while IFS= read -r line; do
        [[ -z "$line" ]] && continue
        IFS=$'\t' read -r label target <<<"$(zellij_ai_parse_machine_spec "$line")" || continue
        if [[ "$needle" == "$label" || "$needle" == "$target" || "$needle" == "$line" ]]; then
            printf '%s\t%s\n' "$label" "$target"
            return 0
        fi
    done < <(zellij_ai_default_machine_specs)

    if IFS=$'\t' read -r label target <<<"$(zellij_ai_parse_machine_spec "$needle" 2>/dev/null)"; then
        printf '%s\t%s\n' "$label" "$target"
        return 0
    fi

    return 1
}

zellij_ai_require_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "Required command not found: $1" >&2
        return 1
    fi
}

zellij_ai_current_machine_name() {
    if [[ -n "${ZELLIJ_AI_MACHINE_NAME:-}" ]]; then
        printf '%s\n' "${ZELLIJ_AI_MACHINE_NAME}"
    else
        hostname -s 2>/dev/null || hostname || printf 'local'
    fi
}

zellij_ai_local_machine_label() {
    local configured
    configured="$(zellij_ai_fleet_config_query local_machine 2>/dev/null || true)"
    if [[ -n "$configured" ]]; then
        printf '%s\n' "$configured"
    else
        printf '%s\n' "command-center"
    fi
}

zellij_ai_agent_path() {
    local path_value="$PATH"

    if [[ -d "$HOME/.local/bin" ]]; then
        path_value="$HOME/.local/bin:$path_value"
    fi
    if [[ -d "$HOME/.npm-global/bin" ]]; then
        path_value="$HOME/.npm-global/bin:$path_value"
    fi

    printf '%s\n' "$path_value"
}

constant_tmux_require() {
    zellij_ai_require_command tmux
}

constant_tmux_session_exists() {
    tmux has-session -t "$1" >/dev/null 2>&1
}

constant_tmux_window_exists() {
    local session_name="$1"
    local window_name="$2"
    tmux list-windows -t "$session_name" -F '#{window_name}' 2>/dev/null | grep -Fxq "$window_name"
}

constant_tmux_set_managed_pane() {
    local pane_id="$1"
    local role="$2"
    local command_string="$3"
    local reset_state="${4:-0}"

    tmux select-pane -t "$pane_id" -T "$role" >/dev/null 2>&1 || true
    tmux set-option -p -t "$pane_id" @constant_role "$role" >/dev/null 2>&1 || true
    tmux set-option -p -t "$pane_id" @constant_command "$command_string" >/dev/null 2>&1 || true
    tmux set-option -p -t "$pane_id" @constant_autorestart 1 >/dev/null 2>&1 || true
    if [[ "$reset_state" == "1" ]]; then
        tmux set-option -p -t "$pane_id" @constant_restart_failures 0 >/dev/null 2>&1 || true
        tmux set-option -p -t "$pane_id" @constant_autorestart_disabled 0 >/dev/null 2>&1 || true
    else
        if [[ -z "$(tmux show-options -p -t "$pane_id" -v @constant_restart_failures 2>/dev/null || true)" ]]; then
            tmux set-option -p -t "$pane_id" @constant_restart_failures 0 >/dev/null 2>&1 || true
        fi
        if [[ -z "$(tmux show-options -p -t "$pane_id" -v @constant_autorestart_disabled 2>/dev/null || true)" ]]; then
            tmux set-option -p -t "$pane_id" @constant_autorestart_disabled 0 >/dev/null 2>&1 || true
        fi
    fi
}

constant_tmux_autorestart_hook_command() {
    cat <<'EOF'
run-shell -b 'sleep 0.8; pane="#{hook_pane}"; dead=$(tmux display-message -p -t "$pane" "#{pane_dead}" 2>/dev/null || true); managed=$(tmux show-options -p -t "$pane" -v @constant_autorestart 2>/dev/null || true); disabled=$(tmux show-options -p -t "$pane" -v @constant_autorestart_disabled 2>/dev/null || true); cmd=$(tmux show-options -p -t "$pane" -v @constant_command 2>/dev/null || true); failures=$(tmux show-options -p -t "$pane" -v @constant_restart_failures 2>/dev/null || printf 0); if [ "$dead" = "1" ] && [ "$managed" = "1" ] && [ "$disabled" != "1" ] && [ -n "$cmd" ]; then failures=$((failures + 1)); tmux set-option -p -t "$pane" @constant_restart_failures "$failures" >/dev/null 2>&1 || true; if [ "$failures" -ge 3 ]; then tmux set-option -p -t "$pane" @constant_autorestart_disabled 1 >/dev/null 2>&1 || true; else tmux respawn-pane -k -t "$pane" "$cmd"; fi; fi'
EOF
}

constant_tmux_configure_autorestart_hook() {
    local session_name="$1"
    tmux set-hook -t "$session_name" pane-died "$(constant_tmux_autorestart_hook_command)" >/dev/null 2>&1 || true
}

constant_tmux_configure_status_chrome() {
    local session_name="$1"
    local left_label="$2"
    local left_bg="${3:-green}"

    tmux set-option -t "$session_name" set-clipboard on >/dev/null 2>&1 || true
    tmux set-option -t "$session_name" remain-on-exit on >/dev/null 2>&1 || true
    tmux set-option -t "$session_name" mouse on >/dev/null 2>&1 || true
    tmux set-option -t "$session_name" status on >/dev/null 2>&1 || true
    tmux set-option -t "$session_name" status-position top >/dev/null 2>&1 || true
    tmux set-option -t "$session_name" status-justify left >/dev/null 2>&1 || true
    tmux set-option -t "$session_name" status-left-length 32 >/dev/null 2>&1 || true
    tmux set-option -t "$session_name" status-right-length 64 >/dev/null 2>&1 || true
    tmux set-option -t "$session_name" status-left "#[fg=colour16,bg=${left_bg},bold] ${left_label} #[default] " >/dev/null 2>&1 || true
    tmux set-option -t "$session_name" status-right '#[fg=colour250]#S #[fg=colour244]| #[fg=colour252]%H:%M #[fg=colour244]%d-%b' >/dev/null 2>&1 || true
    tmux set-option -t "$session_name" status-style 'bg=colour234,fg=colour252' >/dev/null 2>&1 || true
    tmux set-option -t "$session_name" window-status-separator ' ' >/dev/null 2>&1 || true
    tmux set-option -t "$session_name" window-status-format '#[fg=colour245,bg=colour236] #I:#W ' >/dev/null 2>&1 || true
    tmux set-option -t "$session_name" window-status-current-format '#[fg=colour16,bg=colour46,bold] #I:#W ' >/dev/null 2>&1 || true
    tmux set-option -t "$session_name" window-status-current-style 'fg=colour16,bg=colour46,bold' >/dev/null 2>&1 || true
    tmux set-option -t "$session_name" allow-rename off >/dev/null 2>&1 || true
    tmux set-option -t "$session_name" automatic-rename off >/dev/null 2>&1 || true
}

constant_tmux_chat_dock_command() {
    local repo_dir="$1"
    local workspace="$2"
    local scope_label="${3:-}"
    local machine_label="${4:-}"
    local cmd=(
        "$repo_dir/scripts/constant-chat-dock.sh"
        --workspace "$workspace"
    )

    if [[ -n "$scope_label" ]]; then
        cmd+=(--scope-label "$scope_label")
    fi
    if [[ -n "$machine_label" ]]; then
        cmd+=(--machine-label "$machine_label")
    fi

    printf '%q ' "${cmd[@]}"
}

constant_tmux_configure_chat_dock() {
    local session_name="$1"
    local repo_dir="$2"
    local workspace="$3"
    local scope_label="${4:-}"
    local machine_label="${5:-}"
    local dock_cmd dock_line

    dock_cmd="$(constant_tmux_chat_dock_command "$repo_dir" "$workspace" "$scope_label" "$machine_label")"
    dock_line="#[fg=colour16,bg=colour153,bold] Chat #[fg=colour231,bg=colour238] #($dock_cmd)"

    tmux set-option -t "$session_name" status 2 >/dev/null 2>&1 || true
    tmux set-option -t "$session_name" status-interval 4 >/dev/null 2>&1 || true
    tmux set-option -t "$session_name" status-format[1] "$dock_line" >/dev/null 2>&1 || true
}

constant_tmux_base_command() {
    local role="$1"
    local launcher="$2"
    local workspace="$3"
    local repo_dir="$4"
    local session_name="$5"
    local bus_dir="$6"
    local machine_name="$7"
    local codex_home="$8"
    local claude_config_dir="$9"
    local inner

    printf -v inner \
        'cd %q && export ZELLIJ_AI_WORKSPACE=%q ZELLIJ_AI_REPO_DIR=%q ZELLIJ_AI_SESSION=%q ZELLIJ_AI_BUS_DIR=%q ZELLIJ_AI_MACHINE_NAME=%q ZELLIJ_AI_CODEX_HOME=%q ZELLIJ_AI_CODEX_LABEL=%q ZELLIJ_AI_COPILOT_LABEL=%q ZELLIJ_AI_VIBE_LABEL=%q ZELLIJ_AI_ROLE=%q PATH=%q;' \
        "$workspace" \
        "$workspace" \
        "$repo_dir" \
        "$session_name" \
        "$bus_dir" \
        "$machine_name" \
        "$codex_home" \
        "codex" \
        "copilot" \
        "vibe" \
        "$role" \
        "$(zellij_ai_agent_path)"

    if [[ -n "$claude_config_dir" ]]; then
        printf -v inner '%s export ZELLIJ_AI_CLAUDE_CONFIG_DIR=%q;' "$inner" "$claude_config_dir"
    fi

    printf -v inner '%s exec %q' "$inner" "$launcher"
    printf 'bash -lc %q' "$inner"
}

constant_tmux_role_command() {
    local role="$1"
    local script_dir="$2"
    local workspace="$3"
    local repo_dir="$4"
    local session_name="$5"
    local bus_dir="$6"
    local machine_name="$7"
    local codex_home="$8"
    local claude_config_dir="$9"
    local launcher=""

    case "$role" in
        claude)
            launcher="$script_dir/constant-claude-pane.sh"
            ;;
        codex)
            launcher="$script_dir/constant-codex-pane.sh"
            ;;
        copilot)
            launcher="$script_dir/constant-copilot-pane.sh"
            ;;
        vibe)
            launcher="$script_dir/constant-vibe-pane.sh"
            ;;
        *)
            echo "Unknown Constant pane role: $role" >&2
            return 1
            ;;
    esac

    constant_tmux_base_command "$role" "$launcher" "$workspace" "$repo_dir" "$session_name" "$bus_dir" "$machine_name" "$codex_home" "$claude_config_dir"
}
