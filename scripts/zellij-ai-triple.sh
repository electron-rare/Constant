#!/usr/bin/env bash
set -euo pipefail

script_source="${BASH_SOURCE[0]:-$0}"
while [[ -L "$script_source" ]]; do
    script_dir="$(cd "$(dirname "$script_source")" && pwd -P)"
    script_source="$(readlink "$script_source")"
    [[ "$script_source" != /* ]] && script_source="$script_dir/$script_source"
done
script_dir="$(cd "$(dirname "$script_source")" && pwd -P)"
repo_dir="$(cd "$script_dir/.." && pwd -P)"
source "$script_dir/zellij-ai-common.sh"

usage() {
    local script_name="${CONSTANT_SCRIPT_NAME:-$(basename "$0")}"
    cat <<EOF
Usage: ${script_name} [options]

Start a 4-pane Constant machine session:
  - left pane: Claude on the host
  - top-right pane: Codex on the host
  - middle-right pane: Copilot CLI on the host
  - bottom-right pane: Mistral Vibe on the host

Options:
  --workspace DIR       Workspace used by all panes
                        default: current directory
  --session NAME        Session name
                        default: $(zellij_ai_default_session)
  --codex-home DIR      Persistent CODEX_HOME for the Codex pane
                        default: $(zellij_ai_default_codex_home)
  --codex-label LABEL   Display label for the Codex pane
                        default: codex
  --copilot-label LABEL Display label for the Copilot pane
                        default: copilot
  --vibe-label LABEL    Display label for the Vibe pane
                        default: vibe
  --claude-config DIR   Optional CLAUDE_CONFIG_DIR override for the Claude pane
  --zellij-config-dir DIR
                        Optional isolated Zellij config dir for session creation
                        default: a fresh temporary config dir per new session
  --codex-image IMAGE   Deprecated, ignored
  --codex1-home DIR     Deprecated alias for --codex-home
  --codex2-home DIR     Deprecated, ignored
  --codex1-label LABEL  Deprecated alias for --codex-label
  --codex2-label LABEL  Deprecated, ignored
  --recreate            Recreate the session and purge stale Zellij state first
  -h, --help            Show this help
EOF
}

warn_deprecated() {
    printf 'Warning: %s\n' "$*" >&2
}

workspace="$PWD"
session="$(zellij_ai_default_session)"
codex_home="$(zellij_ai_default_codex_home)"
codex_label="codex"
copilot_label="copilot"
vibe_label="vibe"
claude_config_dir=""
zellij_config_dir=""
recreate=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --workspace)
            workspace="$2"
            shift 2
            ;;
        --session)
            session="$2"
            shift 2
            ;;
        --codex-home)
            codex_home="$2"
            shift 2
            ;;
        --codex-label)
            codex_label="$2"
            shift 2
            ;;
        --copilot-label)
            copilot_label="$2"
            shift 2
            ;;
        --vibe-label)
            vibe_label="$2"
            shift 2
            ;;
        --codex-image)
            warn_deprecated "--codex-image is deprecated and ignored; Codex now runs on the host."
            shift 2
            ;;
        --codex1-home)
            warn_deprecated "--codex1-home is deprecated; use --codex-home."
            codex_home="$2"
            shift 2
            ;;
        --codex2-home)
            warn_deprecated "--codex2-home is deprecated and ignored."
            shift 2
            ;;
        --codex1-label)
            warn_deprecated "--codex1-label is deprecated; use --codex-label."
            codex_label="$2"
            shift 2
            ;;
        --codex2-label)
            warn_deprecated "--codex2-label is deprecated and ignored."
            shift 2
            ;;
        --claude-config)
            claude_config_dir="$2"
            shift 2
            ;;
        --zellij-config-dir)
            zellij_config_dir="$2"
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

workspace="$(cd "$workspace" && pwd -P)"
codex_home="$(mkdir -p "$(zellij_ai_expand_home_path "$codex_home")" && cd "$(zellij_ai_expand_home_path "$codex_home")" && pwd -P)"

machine_name="$(zellij_ai_current_machine_name)"
state_dir="$(zellij_ai_session_state_dir "$session")"
bus_dir="$state_dir/bus"
bootstrap_file="$state_dir/bootstrapped"
layout_file="$state_dir/layout.kdl"
clipboard_config_file="$state_dir/zellij.config.kdl"

if [[ ! -d "$workspace" ]]; then
    echo "Workspace not found: $workspace" >&2
    exit 1
fi

for required in \
    "$script_dir/zellij-ai-claude-pane.sh" \
    "$script_dir/zellij-ai-codex-pane.sh" \
    "$script_dir/zellij-ai-copilot-pane.sh" \
    "$script_dir/zellij-ai-vibe-pane.sh" \
    "$script_dir/zellij-ai-bootstrap.sh" \
    "$script_dir/zellij-ai-common.sh"
do
    if [[ ! -x "$required" ]]; then
        echo "Helper script not executable: $required" >&2
        exit 1
    fi
done

mkdir -p "$bus_dir/messages"

if [[ "${ZELLIJ_AI_FORCE_OSC52:-false}" == "true" || "${ZELLIJ_AI_FORCE_OSC52:-0}" == "1" ]]; then
    cat >"$clipboard_config_file" <<'EOF'
copy_clipboard "system"
EOF
elif [[ "$(uname -s)" == "Darwin" && -x /usr/bin/pbcopy ]]; then
    cat >"$clipboard_config_file" <<'EOF'
copy_command "pbcopy"
copy_clipboard "system"
EOF
else
    cat >"$clipboard_config_file" <<'EOF'
copy_clipboard "system"
EOF
fi

export ZELLIJ_AI_WORKSPACE="$workspace"
export ZELLIJ_AI_CODEX_HOME="$codex_home"
export ZELLIJ_AI_CODEX_LABEL="$codex_label"
export ZELLIJ_AI_COPILOT_LABEL="$copilot_label"
export ZELLIJ_AI_VIBE_LABEL="$vibe_label"
export ZELLIJ_AI_MAIN_CODEX_CONFIG="$HOME/.codex/config.toml"
export ZELLIJ_AI_BOOTSTRAP_FILE="$bootstrap_file"
export ZELLIJ_AI_REPO_DIR="$repo_dir"
export ZELLIJ_AI_SESSION="$session"
export ZELLIJ_AI_STATE_DIR="$state_dir"
export ZELLIJ_AI_BUS_DIR="$bus_dir"
export ZELLIJ_AI_MACHINE_NAME="$machine_name"

if [[ -n "$claude_config_dir" ]]; then
    export ZELLIJ_AI_CLAUDE_CONFIG_DIR="$claude_config_dir"
fi

zellij_attach_cmd=(zellij --config "$clipboard_config_file")

if $recreate && zellij_ai_session_exists "$session"; then
    zellij delete-session -f "$session" >/dev/null 2>&1 || true
fi

if zellij_ai_session_exists "$session"; then
    exec "${zellij_attach_cmd[@]}" attach "$session"
fi

zellij delete-session "$session" >/dev/null 2>&1 || true
mkdir -p "$state_dir"
rm -f "$bootstrap_file"

bootstrap_path="$(zellij_ai_kdl_escape "$script_dir/zellij-ai-bootstrap.sh")"
cat >"$layout_file" <<EOF
layout {
    tab name="Constant" {
        pane size=1 borderless=true {
            plugin location="tab-bar"
        }
        pane command="$bootstrap_path"
        pane size=2 borderless=true {
            plugin location="status-bar"
        }
    }
}
EOF

if [[ -z "$zellij_config_dir" ]]; then
    zellij_config_dir="$(mktemp -d "${TMPDIR:-/tmp}/constant-zellij-${session}.XXXXXX")"
else
    zellij_config_dir="$(zellij_ai_expand_home_path "$zellij_config_dir")"
    mkdir -p "$zellij_config_dir"
fi

exec zellij --config "$clipboard_config_file" --config-dir "$zellij_config_dir" --new-session-with-layout "$layout_file" --session "$session"
