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

warn_deprecated() {
    printf 'Warning: %s\n' "$*" >&2
}

usage() {
    local script_name="${CONSTANT_SCRIPT_NAME:-$(basename "$0")}"
    cat <<EOF
Usage: ${script_name} [options]

Open a local Zellij cockpit with one tab per machine.
Each tab attaches to the machine's own local Constant machine session.

Options:
  --machine SPEC        Machine definition. Repeat once per machine.
                        format: [TAB_NAME=]TARGET
                        default fleet: command-center, builder-a, builder-b, edge-a, lab-a
  --session NAME        Remote/local machine session name
                        default: $(zellij_ai_default_session)
  --local-session NAME  Cockpit session name on this machine
                        default: $(zellij_ai_default_local_session)
  --repo-dir DIR        Repository path on each machine
                        default: $(zellij_ai_default_repo_dir)
  --workspace DIR       Workspace path passed to each machine session
                        required for the fixed 5-machine fleet
  --codex-image IMAGE   Deprecated, ignored
  --claude-config DIR   Optional Claude config override passed to each machine
  --remote-recreate     Recreate each machine session before attaching
  --recreate            Recreate the local cockpit session before launching
  -h, --help            Show this help
EOF
}

machines=()
remote_session="$(zellij_ai_default_session)"
local_session="$(zellij_ai_default_local_session)"
remote_repo_dir="$(zellij_ai_default_repo_dir)"
workspace=""
codex_image=""
claude_config_dir=""
recreate=false
remote_recreate=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --machine)
            machines+=("$2")
            shift 2
            ;;
        --session)
            remote_session="$2"
            shift 2
            ;;
        --local-session)
            local_session="$2"
            shift 2
            ;;
        --repo-dir)
            remote_repo_dir="$2"
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

if [[ ${#machines[@]} -eq 0 ]]; then
    mapfile -t machines < <(zellij_ai_default_machine_specs)
fi

if [[ -z "$workspace" ]]; then
    echo "--workspace is required for fleet mode." >&2
    exit 2
fi

workspace="$(cd "$workspace" && pwd -P)"

for required in \
    zellij \
    ssh \
    "$script_dir/zellij-ai-common.sh" \
    "$script_dir/zellij-ai-fleet-bootstrap.sh" \
    "$script_dir/zellij-ai-remote-tab.sh"
do
    if [[ "$required" == */* ]]; then
        if [[ ! -x "$required" ]]; then
            echo "Helper script not executable: $required" >&2
            exit 1
        fi
    else
        zellij_ai_require_command "$required"
    fi
done

state_dir="$(zellij_ai_fleet_state_dir "$local_session")"
tabs_file="$state_dir/tabs.tsv"
tabs_created_file="$state_dir/tabs-created"
first_layout_file="$state_dir/first-tab.kdl"
mkdir -p "$state_dir"
: >"$tabs_file"
rm -f "$tabs_created_file"

bootstrap_path="$(zellij_ai_kdl_escape "$script_dir/zellij-ai-fleet-bootstrap.sh")"

first_label=""
first_wrapper=""
seen_labels='|'
machine_index=0

for machine_spec in "${machines[@]}"; do
    if ! parsed_spec="$(zellij_ai_parse_machine_spec "$machine_spec")"; then
        echo "Invalid machine spec: $machine_spec" >&2
        exit 2
    fi
    IFS=$'\t' read -r machine_label machine_target <<<"$parsed_spec"

    if [[ "$seen_labels" == *"|${machine_label}|"* ]]; then
        echo "Duplicate machine label: $machine_label" >&2
        exit 2
    fi
    seen_labels+="${machine_label}|"

    wrapper_path="$state_dir/machine-${machine_index}.sh"
    layout_path="$state_dir/machine-${machine_index}.kdl"

    wrapper_cmd=(
        "$script_dir/zellij-ai-remote-tab.sh"
        --label "$machine_label"
        --target "$machine_target"
        --session "$remote_session"
        --repo-dir "$remote_repo_dir"
    )

    if [[ -n "$workspace" ]]; then
        wrapper_cmd+=(--workspace "$workspace")
    fi
    if [[ -n "$codex_image" ]]; then
        wrapper_cmd+=(--codex-image "$codex_image")
    fi
    if [[ -n "$claude_config_dir" ]]; then
        wrapper_cmd+=(--claude-config "$claude_config_dir")
    fi
    if $remote_recreate; then
        wrapper_cmd+=(--remote-recreate)
    fi

    printf -v wrapper_exec '%q ' "${wrapper_cmd[@]}"
    wrapper_exec="${wrapper_exec% }"

    cat >"$wrapper_path" <<EOF
#!/usr/bin/env bash
set -euo pipefail
exec $wrapper_exec
EOF
    chmod +x "$wrapper_path"

    wrapper_path_escaped="$(zellij_ai_kdl_escape "$wrapper_path")"
    cat >"$layout_path" <<EOF
layout {
    pane size=1 borderless=true {
        plugin location="tab-bar"
    }
    pane command="$wrapper_path_escaped"
    pane size=2 borderless=true {
        plugin location="status-bar"
    }
}
EOF

    if [[ $machine_index -eq 0 ]]; then
        first_label="$machine_label"
        first_wrapper="$wrapper_path"
    else
        printf '%s\t%s\t%s\n' "$machine_label" "$layout_path" "$repo_dir" >>"$tabs_file"
    fi

    machine_index=$((machine_index + 1))
done

if [[ -z "$first_label" || -z "$first_wrapper" ]]; then
    echo "No machine tabs were generated" >&2
    exit 1
fi

cat >"$first_layout_file" <<EOF
layout {
    pane size=1 borderless=true {
        plugin location="tab-bar"
    }
    pane command="$bootstrap_path"
    pane size=2 borderless=true {
        plugin location="status-bar"
    }
}
EOF

export ZELLIJ_AI_FLEET_TABS_FILE="$tabs_file"
export ZELLIJ_AI_FLEET_CREATED_FILE="$tabs_created_file"
export ZELLIJ_AI_FLEET_CURRENT_LABEL="$first_label"
export ZELLIJ_AI_FLEET_CURRENT_WRAPPER="$first_wrapper"

if $recreate && zellij_ai_session_exists "$local_session"; then
    zellij kill-session "$local_session" >/dev/null 2>&1 || true
fi

if zellij_ai_session_exists "$local_session"; then
    exec zellij attach "$local_session"
fi

exec zellij --new-session-with-layout "$first_layout_file" --session "$local_session"
