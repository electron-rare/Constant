#!/usr/bin/env bash
set -euo pipefail

script_source="${BASH_SOURCE[0]:-$0}"
while [[ -L "$script_source" ]]; do
    script_dir="$(cd "$(dirname "$script_source")" && pwd -P)"
    script_source="$(readlink "$script_source")"
    [[ "$script_source" != /* ]] && script_source="$script_dir/$script_source"
done
script_dir="$(cd "$(dirname "$script_source")" && pwd -P)"
repo_source="$(cd "$script_dir/.." && pwd -P)"
source "$script_dir/zellij-ai-common.sh"

usage() {
    local script_name="${CONSTANT_SCRIPT_NAME:-$(basename "$0")}"
    cat <<EOF
Usage: ${script_name} <check|install> [options]

Check or install the Constant fleet runtime on one or more machines.

Options:
  --machine SPEC        Machine definition or label to target
  --repo-dir DIR        Repository path on each machine
                        default: $(zellij_ai_default_repo_dir)
  --codex-image IMAGE   Deprecated, ignored
  --yes                 Required for non-interactive install mode
  -h, --help            Show this help
EOF
}

warn_deprecated() {
    printf '[fleet-install] %s\n' "$*" >&2
}

require_local_tools() {
    local tool
    for tool in bash ssh tar rsync; do
        zellij_ai_require_command "$tool"
    done
}

remote_home() {
    ssh "$1" 'printf "%s\n" "$HOME"'
}

remote_repo_dir() {
    local target="$1"
    local repo_dir_input="$2"
    local target_home

    if zellij_ai_is_local_target "$target"; then
        zellij_ai_expand_home_path "$repo_dir_input"
        return 0
    fi

    target_home="$(remote_home "$target")"
    zellij_ai_expand_home_path_for_home "$repo_dir_input" "$target_home"
}

sync_repo_to_remote() {
    local target="$1"
    local repo_dir_input="$2"
    local target_repo_dir local_real target_real

    target_repo_dir="$(remote_repo_dir "$target" "$repo_dir_input")"

    if zellij_ai_is_local_target "$target"; then
        mkdir -p "$target_repo_dir"
        local_real="$repo_source"
        target_real="$(cd "$target_repo_dir" && pwd -P)"
        if [[ "$local_real" == "$target_real" ]]; then
            return 0
        fi
        rsync -a --delete --exclude '.git/' "$repo_source/" "$target_repo_dir/"
        return 0
    fi

    ssh "$target" "mkdir -p $(printf '%q' "$target_repo_dir")"

    if ssh "$target" 'command -v rsync >/dev/null 2>&1'; then
        rsync -a --delete --exclude '.git/' "$repo_source/" "${target}:${target_repo_dir}/"
    else
        (
            cd "$repo_source"
            tar --exclude '.git' -cf - .
        ) | ssh "$target" "tar -xf - -C $(printf '%q' "$target_repo_dir")"
    fi
}

mode="${1:-}"
if [[ -z "$mode" ]]; then
    usage >&2
    exit 2
fi

case "$mode" in
    -h|--help|help)
        usage
        exit 0
        ;;
esac

shift || true

machines=()
repo_dir="$(zellij_ai_default_repo_dir)"
yes=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --machine)
            machines+=("$2")
            shift 2
            ;;
        --repo-dir)
            repo_dir="$2"
            shift 2
            ;;
        --codex-image)
            warn_deprecated "--codex-image is deprecated and ignored."
            shift 2
            ;;
        --yes)
            yes=true
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

case "$mode" in
    check|install)
        ;;
    *)
        echo "Unknown mode: $mode" >&2
        usage >&2
        exit 2
        ;;
esac

require_local_tools

if [[ ${#machines[@]} -eq 0 ]]; then
    mapfile -t machines < <(zellij_ai_default_machine_specs)
fi

overall_status=0

for machine_spec in "${machines[@]}"; do
    if ! resolved="$(zellij_ai_resolve_machine_spec "$machine_spec")"; then
        echo "Unknown machine: $machine_spec" >&2
        overall_status=1
        continue
    fi

    IFS=$'\t' read -r label target <<<"$resolved"
    printf '=== %s (%s) ===\n' "$label" "$target"

    target_repo_dir="$(remote_repo_dir "$target" "$repo_dir")"

    if [[ "$mode" == "install" ]]; then
        sync_repo_to_remote "$target" "$repo_dir" || overall_status=1
    fi

    install_args=("$mode" --repo-dir "$repo_dir")
    if $yes; then
        install_args+=(--yes)
    fi

    if zellij_ai_is_local_target "$target"; then
        ZELLIJ_AI_MACHINE_NAME="$label" "$script_dir/zellij-ai-machine-install.sh" "${install_args[@]}" || overall_status=1
        continue
    fi

    if [[ "$mode" == "install" ]]; then
        remote_script="${target_repo_dir}/scripts/zellij-ai-machine-install.sh"
        ssh -tt "$target" env ZELLIJ_AI_MACHINE_NAME="$label" "$remote_script" "${install_args[@]}" || overall_status=1
        continue
    fi

    if $yes; then
        ssh "$target" env ZELLIJ_AI_MACHINE_NAME="$label" bash -s -- "${install_args[@]}" <"$script_dir/zellij-ai-machine-install.sh" || overall_status=1
    else
        ssh "$target" env ZELLIJ_AI_MACHINE_NAME="$label" bash -s -- "${install_args[@]}" <"$script_dir/zellij-ai-machine-install.sh" || overall_status=1
    fi
done

exit "$overall_status"
