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
    cat <<'EOF'
Usage: ai-msg.sh <command> [options]

Commands:
  send       Send a message to one role
  broadcast  Send a message to all roles on the current machine
  inbox      Show messages addressed to one role
  tail       Poll and print new messages for one role
  list       List matching messages in TSV form

Environment defaults:
  ZELLIJ_AI_BUS_DIR
  ZELLIJ_AI_MACHINE_NAME
  ZELLIJ_AI_ROLE
  ZELLIJ_AI_SESSION
EOF
}

machine_name="$(zellij_ai_current_machine_name)"
session="${ZELLIJ_AI_SESSION:-$(zellij_ai_default_session)}"
role_default="${ZELLIJ_AI_ROLE:-all}"
bus_dir="${ZELLIJ_AI_BUS_DIR:-$(zellij_ai_session_state_dir "$session")/bus}"
messages_dir="${bus_dir}/messages"

ensure_messages_dir() {
    mkdir -p "$messages_dir"
}

random_suffix() {
    od -An -N4 -tx1 /dev/urandom 2>/dev/null | tr -d ' \n' || printf '%s' "$$"
}

message_id() {
    printf '%s\n' "${machine_name}-${session}-$(date -u +%Y%m%dT%H%M%SZ)-$$-$(random_suffix)"
}

timestamp_utc() {
    date -u +"%Y-%m-%dT%H:%M:%SZ"
}

read_body_input() {
    local message_text="$1"
    local message_file="$2"

    if [[ -n "$message_text" ]]; then
        printf '%s' "$message_text"
        return 0
    fi

    if [[ -n "$message_file" ]]; then
        cat "$message_file"
        return 0
    fi

    if [[ ! -t 0 ]]; then
        cat
        return 0
    fi

    echo "No message body provided. Use --message, --file, or stdin." >&2
    return 1
}

read_header() {
    local key="$1"
    local file="$2"
    sed -n "s/^${key}: //p" "$file" | head -n 1
}

read_body() {
    awk 'seen_blank { print } /^$/ { seen_blank = 1 }' "$1"
}

message_matches_role() {
    local file="$1"
    local role="$2"
    local target

    target="$(read_header target "$file")"

    if [[ "$role" == "all" ]]; then
        return 0
    fi

    [[ "$target" == "$role" || "$target" == "all" ]]
}

list_message_files() {
    if [[ ! -d "$messages_dir" ]]; then
        return 0
    fi

    find "$messages_dir" -type f -name '*.msg' | sort
}

print_message_tsv() {
    local file="$1"
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$(read_header id "$file")" \
        "$(read_header timestamp "$file")" \
        "$(read_header machine "$file")" \
        "$(read_header session "$file")" \
        "$(read_header sender "$file")" \
        "$(read_header target "$file")" \
        "$(read_header source_machine "$file")" \
        "$(read_header source_role "$file")" \
        "$file"
}

print_message_pretty() {
    local file="$1"
    local id timestamp sender target stored_machine source_machine source_role relay_via

    id="$(read_header id "$file")"
    timestamp="$(read_header timestamp "$file")"
    sender="$(read_header sender "$file")"
    target="$(read_header target "$file")"
    stored_machine="$(read_header machine "$file")"
    source_machine="$(read_header source_machine "$file")"
    source_role="$(read_header source_role "$file")"
    relay_via="$(read_header relay_via "$file")"

    printf '[%s] %s -> %s on %s (%s)\n' "$timestamp" "$sender" "$target" "$stored_machine" "$id"
    if [[ -n "$source_machine" || -n "$source_role" ]]; then
        printf 'source: %s/%s\n' "${source_machine:-unknown}" "${source_role:-unknown}"
    fi
    if [[ -n "$relay_via" ]]; then
        printf 'relay: %s\n' "$relay_via"
    fi
    printf '%s\n' '---'
    read_body "$file"
    printf '\n'
}

write_message_file() {
    local sender="$1"
    local target="$2"
    local source_machine="$3"
    local source_role="$4"
    local relay_via="$5"
    local body="$6"
    local id timestamp file tmp_file

    ensure_messages_dir

    id="$(message_id)"
    timestamp="$(timestamp_utc)"
    file="${messages_dir}/${id}.msg"
    tmp_file="$(mktemp "${messages_dir}/.tmp.XXXXXX")"

    {
        printf 'id: %s\n' "$id"
        printf 'timestamp: %s\n' "$timestamp"
        printf 'machine: %s\n' "$machine_name"
        printf 'session: %s\n' "$session"
        printf 'sender: %s\n' "$sender"
        printf 'target: %s\n' "$target"
        printf 'source_machine: %s\n' "$source_machine"
        printf 'source_role: %s\n' "$source_role"
        printf 'relay_via: %s\n' "$relay_via"
        printf '\n'
        printf '%s\n' "$body"
    } >"$tmp_file"

    mv "$tmp_file" "$file"
    printf '%s\n' "$file"
}

command_send() {
    local sender="$role_default"
    local target=""
    local message_text=""
    local message_file=""
    local source_machine=""
    local source_role=""
    local relay_via=""
    local body file

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --from)
                sender="$2"
                shift 2
                ;;
            --to)
                target="$2"
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
            --source-machine)
                source_machine="$2"
                shift 2
                ;;
            --source-role)
                source_role="$2"
                shift 2
                ;;
            --relay-via)
                relay_via="$2"
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

    if [[ -z "$target" ]]; then
        echo "--to is required for send." >&2
        exit 2
    fi

    body="$(read_body_input "$message_text" "$message_file")"
    file="$(write_message_file "$sender" "$target" "$source_machine" "$source_role" "$relay_via" "$body")"
    printf 'stored: %s\n' "$file"
}

command_broadcast() {
    command_send --to all "$@"
}

command_list() {
    local role="all"

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --for)
                role="$2"
                shift 2
                ;;
            -h|--help)
                usage
                exit 0
                ;;
            *)
                echo "Unknown option for list: $1" >&2
                exit 2
                ;;
        esac
    done

    while IFS= read -r file; do
        message_matches_role "$file" "$role" || continue
        print_message_tsv "$file"
    done < <(list_message_files)
}

command_inbox() {
    local role="$role_default"
    local found=false

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --for)
                role="$2"
                shift 2
                ;;
            -h|--help)
                usage
                exit 0
                ;;
            *)
                echo "Unknown option for inbox: $1" >&2
                exit 2
                ;;
        esac
    done

    while IFS= read -r file; do
        message_matches_role "$file" "$role" || continue
        print_message_pretty "$file"
        found=true
    done < <(list_message_files)

    if ! $found; then
        printf 'No messages for %s in %s\n' "$role" "$messages_dir"
    fi
}

command_tail() {
    local role="$role_default"
    local interval=2
    declare -A seen=()

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --for)
                role="$2"
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
        while IFS= read -r file; do
            message_matches_role "$file" "$role" || continue
            if [[ -n "${seen[$file]:-}" ]]; then
                continue
            fi
            seen["$file"]=1
            print_message_pretty "$file"
        done < <(list_message_files)
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
    list)
        command_list "$@"
        ;;
    inbox)
        command_inbox "$@"
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
