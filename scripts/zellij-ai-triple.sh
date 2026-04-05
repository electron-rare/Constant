#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
repo_dir="$(cd "$script_dir/.." && pwd -P)"

usage() {
    cat <<EOF
Usage: $(basename "$0") [options]

Start a 3-pane Zellij session:
  - left pane: Claude on the host
  - top-right pane: Codex in Docker with profile 1
  - bottom-right pane: Codex in Docker with profile 2

Options:
  --workspace DIR       Workspace mounted in both Codex containers and used by Claude
                        default: current directory
  --session NAME        Zellij session name
                        default: ai-triple
  --codex-image IMAGE   Docker image used for Codex panes
                        default: codercom/code-server:latest
  --codex1-home DIR     Persistent CODEX_HOME for the first Codex pane
                        default: \$HOME/.codex-profiles/codex-1
  --codex2-home DIR     Persistent CODEX_HOME for the second Codex pane
                        default: \$HOME/.codex-profiles/codex-2
  --codex1-label LABEL  Display label for the first Codex pane
                        default: codex-1
  --codex2-label LABEL  Display label for the second Codex pane
                        default: codex-2
  --claude-config DIR   Optional CLAUDE_CONFIG_DIR override for the Claude pane
  --zellij-config-dir DIR
                        Optional isolated Zellij config dir to use for session creation
                        default: a fresh temporary config dir per new session
  --recreate            Kill the existing session before recreating it
  -h, --help            Show this help
EOF
}

strip_ansi() {
    sed -E 's/\x1b\[[0-9;]*m//g'
}

session_exists() {
    zellij list-sessions 2>/dev/null | strip_ansi | awk '{print $1}' | grep -Fxq "$1"
}

kdl_escape() {
    printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

workspace="$PWD"
session="ai-triple"
codex_image="codercom/code-server:latest"
codex1_home="$HOME/.codex-profiles/codex-1"
codex2_home="$HOME/.codex-profiles/codex-2"
codex1_label="codex-1"
codex2_label="codex-2"
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
        --codex-image)
            codex_image="$2"
            shift 2
            ;;
        --codex1-home)
            codex1_home="$2"
            shift 2
            ;;
        --codex2-home)
            codex2_home="$2"
            shift 2
            ;;
        --codex1-label)
            codex1_label="$2"
            shift 2
            ;;
        --codex2-label)
            codex2_label="$2"
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
codex1_home="$(mkdir -p "$codex1_home" && cd "$codex1_home" && pwd -P)"
codex2_home="$(mkdir -p "$codex2_home" && cd "$codex2_home" && pwd -P)"

bootstrap_file="$HOME/.cache/zellij-ai/${session}.bootstrapped"
layout_file="/tmp/zellij-ai-layout-${session}.kdl"

if [[ ! -d "$workspace" ]]; then
    echo "Workspace not found: $workspace" >&2
    exit 1
fi

for required in \
    "$script_dir/zellij-ai-claude-pane.sh" \
    "$script_dir/zellij-ai-codex-pane.sh" \
    "$script_dir/zellij-ai-bootstrap.sh"
do
    if [[ ! -x "$required" ]]; then
        echo "Helper script not executable: $required" >&2
        exit 1
    fi
done

export ZELLIJ_AI_WORKSPACE="$workspace"
export ZELLIJ_AI_CODEX_IMAGE="$codex_image"
export ZELLIJ_AI_CODEX1_HOME="$codex1_home"
export ZELLIJ_AI_CODEX2_HOME="$codex2_home"
export ZELLIJ_AI_CODEX1_LABEL="$codex1_label"
export ZELLIJ_AI_CODEX2_LABEL="$codex2_label"
export ZELLIJ_AI_MAIN_CODEX_CONFIG="$HOME/.codex/config.toml"
export ZELLIJ_AI_BOOTSTRAP_FILE="$bootstrap_file"

if [[ -n "$claude_config_dir" ]]; then
    export ZELLIJ_AI_CLAUDE_CONFIG_DIR="$claude_config_dir"
fi

if $recreate && session_exists "$session"; then
    zellij delete-session -f "$session" >/dev/null 2>&1 || true
fi

if session_exists "$session"; then
    exec zellij attach "$session"
fi

zellij delete-session "$session" >/dev/null 2>&1 || true

mkdir -p "$(dirname "$bootstrap_file")"
rm -f "$bootstrap_file"

bootstrap_path="$(kdl_escape "$script_dir/zellij-ai-bootstrap.sh")"
cat >"$layout_file" <<EOF
layout {
    tab name="AI Triple" {
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
    zellij_config_dir="$(mktemp -d "${TMPDIR:-/tmp}/zellij-ai-triple-${session}.XXXXXX")"
else
    mkdir -p "$zellij_config_dir"
fi

exec zellij --config-dir "$zellij_config_dir" --new-session-with-layout "$layout_file" --session "$session"
