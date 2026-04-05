#!/usr/bin/env bash
set -euo pipefail

script_source="${BASH_SOURCE[0]:-$0}"
while [[ -L "$script_source" ]]; do
    script_dir="$(cd "$(dirname "$script_source")" && pwd -P)"
    script_source="$(readlink "$script_source")"
    [[ "$script_source" != /* ]] && script_source="$script_dir/$script_source"
done
script_dir="$(cd "$(dirname "$script_source")" && pwd -P)"
source "$script_dir/zellij-ai-common.sh"

usage() {
    cat <<EOF
Usage: $(basename "$0") <command> [options]

Commands:
  send       Relay one message to one machine
  broadcast  Relay one message to all machines
  sync       Refresh a local cache of remote inbox summaries
  tail       Poll all machines and print new summaries

Defaults:
  repo-dir: $(zellij_ai_default_repo_dir)
  session:  $(zellij_ai_default_session)
EOF
}

repo_dir="$(zellij_ai_default_repo_dir)"
session="$(zellij_ai_default_session)"
cache_dir="$(zellij_ai_bridge_cache_dir)"
machines=()

default_machine_specs_array() {
    local line
    while IFS= read -r line; do
        [[ -z "$line" ]] && continue
        printf '%s\n' "$line"
    done < <(zellij_ai_default_machine_specs)
}

load_machines() {
    if [[ ${#machines[@]} -eq 0 ]]; then
        mapfile -t machines < <(default_machine_specs_array)
    fi
}

resolve_machine() {
    local needle="$1"
    local resolved

    if ! resolved="$(zellij_ai_resolve_machine_spec "$needle")"; then
        echo "Unknown machine: $needle" >&2
        exit 2
    fi

    printf '%s\n' "$resolved"
}

run_ai_msg() {
    local label="$1"
    local target="$2"
    shift 2

    if zellij_ai_is_local_target "$target"; then
        ZELLIJ_AI_MACHINE_NAME="$label" \
        ZELLIJ_AI_SESSION="$session" \
        "$(zellij_ai_expand_home_path "$repo_dir")/scripts/ai-msg.sh" "$@"
        return 0
    fi

    ssh "$target" bash -s -- "$label" "$session" "$repo_dir" "$@" <<'EOF'
set -euo pipefail

label="$1"
shift
session="$1"
shift
repo_dir_input="$1"
shift

expand_home_path() {
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

repo_dir="$(expand_home_path "$repo_dir_input")"

export ZELLIJ_AI_MACHINE_NAME="$label"
export ZELLIJ_AI_SESSION="$session"

exec "$repo_dir/scripts/ai-msg.sh" "$@"
EOF
}

sync_once() {
    local machine_spec resolved label target cache_file
    mkdir -p "$cache_dir"
    load_machines

    for machine_spec in "${machines[@]}"; do
        IFS=$'\t' read -r label target <<<"$(resolve_machine "$machine_spec")"
        cache_file="$cache_dir/${label}.tsv"
        run_ai_msg "$label" "$target" list --for all >"$cache_file"
    done
}

command_send() {
    local from_machine="$(zellij_ai_local_machine_label)"
    local from_role="bridge"
    local to_machine=""
    local to_role=""
    local message_text=""
    local message_file=""
    local resolved label target

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --from-machine)
                from_machine="$2"
                shift 2
                ;;
            --from)
                from_role="$2"
                shift 2
                ;;
            --to-machine)
                to_machine="$2"
                shift 2
                ;;
            --to)
                to_role="$2"
                shift 2
                ;;
            --message)
                message_text="$2"
                shift 2
                ;;
            --file)
                message_file="$2"
                shift 2
                ;;
            --machine)
                machines+=("$2")
                shift 2
                ;;
            --repo-dir)
                repo_dir="$2"
                shift 2
                ;;
            --session)
                session="$2"
                shift 2
                ;;
            -h|--help)
                usage
                exit 0
                ;;
            *)
                echo "Unknown option for send: $1" >&2
                exit 2
                ;;
        esac
    done

    if [[ -z "$to_machine" || -z "$to_role" ]]; then
        echo "--to-machine and --to are required." >&2
        exit 2
    fi

    IFS=$'\t' read -r label target <<<"$(resolve_machine "$to_machine")"

    if [[ -n "$message_text" ]]; then
        run_ai_msg "$label" "$target" send --from "$from_role" --to "$to_role" --source-machine "$from_machine" --source-role "$from_role" --relay-via "$(zellij_ai_local_machine_label)" --message "$message_text"
    elif [[ -n "$message_file" ]]; then
        run_ai_msg "$label" "$target" send --from "$from_role" --to "$to_role" --source-machine "$from_machine" --source-role "$from_role" --relay-via "$(zellij_ai_local_machine_label)" --file "$message_file"
    elif [[ ! -t 0 ]]; then
        run_ai_msg "$label" "$target" send --from "$from_role" --to "$to_role" --source-machine "$from_machine" --source-role "$from_role" --relay-via "$(zellij_ai_local_machine_label)"
    else
        echo "Provide --message, --file, or stdin." >&2
        exit 2
    fi
}

command_broadcast() {
    local from_machine="$(zellij_ai_local_machine_label)"
    local from_role="bridge"
    local message_text=""
    local message_file=""
    local machine_spec resolved label target

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --from-machine)
                from_machine="$2"
                shift 2
                ;;
            --from)
                from_role="$2"
                shift 2
                ;;
            --message)
                message_text="$2"
                shift 2
                ;;
            --file)
                message_file="$2"
                shift 2
                ;;
            --machine)
                machines+=("$2")
                shift 2
                ;;
            --repo-dir)
                repo_dir="$2"
                shift 2
                ;;
            --session)
                session="$2"
                shift 2
                ;;
            -h|--help)
                usage
                exit 0
                ;;
            *)
                echo "Unknown option for broadcast: $1" >&2
                exit 2
                ;;
        esac
    done

    load_machines

    for machine_spec in "${machines[@]}"; do
        IFS=$'\t' read -r label target <<<"$(resolve_machine "$machine_spec")"
        if [[ -n "$message_text" ]]; then
            run_ai_msg "$label" "$target" send --from "$from_role" --to all --source-machine "$from_machine" --source-role "$from_role" --relay-via "$(zellij_ai_local_machine_label)" --message "$message_text"
        elif [[ -n "$message_file" ]]; then
            run_ai_msg "$label" "$target" send --from "$from_role" --to all --source-machine "$from_machine" --source-role "$from_role" --relay-via "$(zellij_ai_local_machine_label)" --file "$message_file"
        elif [[ ! -t 0 ]]; then
            run_ai_msg "$label" "$target" send --from "$from_role" --to all --source-machine "$from_machine" --source-role "$from_role" --relay-via "$(zellij_ai_local_machine_label)"
        else
            echo "Provide --message, --file, or stdin." >&2
            exit 2
        fi
    done
}

command_sync() {
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
            --session)
                session="$2"
                shift 2
                ;;
            -h|--help)
                usage
                exit 0
                ;;
            *)
                echo "Unknown option for sync: $1" >&2
                exit 2
                ;;
        esac
    done

    sync_once

    find "$cache_dir" -type f -name '*.tsv' | sort | while IFS= read -r file; do
        cat "$file"
    done
}

command_tail() {
    local interval=3
    declare -A seen=()

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
            --session)
                session="$2"
                shift 2
                ;;
            --interval)
                interval="$2"
                shift 2
                ;;
            -h|--help)
                usage
                exit 0
                ;;
            *)
                echo "Unknown option for tail: $1" >&2
                exit 2
                ;;
        esac
    done

    while true; do
        sync_once
        while IFS= read -r file; do
            while IFS= read -r line; do
                [[ -z "$line" ]] && continue
                if [[ -n "${seen[$line]:-}" ]]; then
                    continue
                fi
                seen["$line"]=1
                printf '%s\n' "$line"
            done <"$file"
        done < <(find "$cache_dir" -type f -name '*.tsv' | sort)
        sleep "$interval"
    done
}

if [[ $# -lt 1 ]]; then
    usage >&2
    exit 2
fi

command="$1"
shift

case "$command" in
    send)
        command_send "$@"
        ;;
    broadcast)
        command_broadcast "$@"
        ;;
    sync)
        command_sync "$@"
        ;;
    tail)
        command_tail "$@"
        ;;
    -h|--help|help)
        usage
        ;;
    *)
        echo "Unknown command: $command" >&2
        usage >&2
        exit 2
        ;;
esac
