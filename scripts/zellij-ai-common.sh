#!/usr/bin/env bash

zellij_ai_config_root() {
    printf '%s\n' "${CONSTANT_CONFIG_ROOT:-$HOME/.config/constant}"
}

zellij_ai_fleet_config_candidates() {
    if [[ -n "${CONSTANT_FLEET_CONFIG:-}" ]]; then
        printf '%s\n' "$CONSTANT_FLEET_CONFIG"
    fi
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

    config_path="$(zellij_ai_fleet_config_path 2>/dev/null || true)"
    [[ -n "$config_path" ]] || return 1
    command -v python3 >/dev/null 2>&1 || return 1

    python3 - "$config_path" "$expr" <<'PY'
import json
import sys
from pathlib import Path

path = Path(sys.argv[1])
expr = sys.argv[2]

try:
    payload = json.loads(path.read_text(encoding="utf-8"))
except Exception:
    raise SystemExit(1)

if expr == "repo_dir":
    value = payload.get("repo_dir")
    if isinstance(value, str) and value:
        print(value)
    raise SystemExit(0)

if expr == "local_machine":
    value = payload.get("local_machine")
    if isinstance(value, str) and value:
        print(value)
    raise SystemExit(0)

if expr == "machine_specs":
    for machine in payload.get("machines", []):
        label = machine.get("label")
        target = machine.get("target")
        if isinstance(label, str) and label and isinstance(target, str) and target:
            print(f"{label}={target}")
    raise SystemExit(0)

raise SystemExit(1)
PY
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
    printf '%s\n' "${ZELLIJ_AI_RUNTIME_ROOT:-$HOME/.cache/constant/zellij}"
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
    zellij list-sessions 2>/dev/null | zellij_ai_strip_ansi | awk '{print $1}' | grep -Fxq "$1"
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
