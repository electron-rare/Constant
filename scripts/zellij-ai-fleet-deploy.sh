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
    local script_name="${CONSTANT_SCRIPT_NAME:-$(basename "$0")}"
    cat <<EOF
Usage: ${script_name} [scan|configure|deploy] [options]

Discover SSH-capable machines, select targets, write Constant fleet config,
and optionally deploy the runtime.

Modes:
  scan       Discover candidates and print them
  configure  Discover, select, and write fleet.json (default)
  deploy     Discover, select, write fleet.json, then run install

Options:
  --host HOST            Add an explicit SSH host, alias, or IP. Repeatable.
  --user USER            Default SSH user override for remote machines
  --repo-dir DIR         Repo path to deploy/use on targets
                         default: $(zellij_ai_default_repo_dir)
  --local-label LABEL    Label for the local command-center machine
                         default: current hostname
  --output PATH          Fleet config output path
                         default: $(zellij_ai_config_root)/fleet.json
  --json                 Print scan output as JSON
  --yes                  Non-interactive mode; select all reachable machines
  --all-reachable        In interactive mode, preselect all reachable machines
  --no-ssh-config        Skip discovery from ~/.ssh/config
  --no-known-hosts       Skip discovery from ~/.ssh/known_hosts
  --no-arp               Skip discovery from arp neighbors
  --install              Run install after writing config
  -h, --help             Show this help
EOF
}

log() {
    printf '[deploy] %s\n' "$*" >&2
}

fail() {
    printf '[deploy] %s\n' "$*" >&2
    exit 1
}

use_gum() {
    [[ -t 0 && -t 1 ]] && command -v gum >/dev/null 2>&1
}

current_local_label() {
    hostname -s 2>/dev/null || hostname || printf 'local'
}

sanitize_label() {
    printf '%s' "$1" | tr '[:upper:]' '[:lower:]' | sed -E 's/[^a-z0-9._-]+/-/g; s/^-+//; s/-+$//; s/--+/-/g'
}

unique_lines() {
    local source_file="$1"
    local dest_file="$2"
    if [[ -s "$source_file" ]]; then
        sort -u "$source_file" >"$dest_file"
    else
        : >"$dest_file"
    fi
}

scan_ssh_config() {
    local output_file="$1"
    local config_file="$HOME/.ssh/config"
    [[ -f "$config_file" ]] || return 0

    awk '
        BEGIN { IGNORECASE = 1 }
        $1 == "Host" {
            for (i = 2; i <= NF; i++) {
                if ($i ~ /[*?!]/) {
                    continue
                }
                print $i
            }
        }
    ' "$config_file" >>"$output_file"
}

scan_known_hosts() {
    local output_file="$1"
    local known_hosts="$HOME/.ssh/known_hosts"
    [[ -f "$known_hosts" ]] || return 0

    awk -F'[ ,]' '
        /^[|#]/ { next }
        NF < 1 { next }
        {
            split($1, hosts, ",")
            for (i in hosts) {
                host = hosts[i]
                if (host ~ /^\[/) {
                    sub(/^\[/, "", host)
                    sub(/\]:[0-9]+$/, "", host)
                }
                if (host !~ /^[|#]/ && host != "") {
                    print host
                }
            }
        }
    ' "$known_hosts" >>"$output_file"
}

scan_arp_neighbors() {
    local output_file="$1"
    command -v arp >/dev/null 2>&1 || return 0
    arp -a 2>/dev/null | sed -nE 's/.*\(([0-9a-fA-F:.]+)\).*/\1/p' >>"$output_file"
}

resolve_seed() {
    local seed="$1"
    local default_user="$2"
    local resolved_user resolved_host resolved_port lookup_seed explicit_user

    resolved_user="$default_user"
    resolved_host="$seed"
    resolved_port="22"
    lookup_seed="$seed"
    explicit_user=""

    if [[ "$seed" == *@* ]]; then
        explicit_user="${seed%@*}"
        lookup_seed="${seed#*@}"
        if [[ -n "$explicit_user" && -n "$lookup_seed" ]]; then
            resolved_user="$explicit_user"
            resolved_host="$lookup_seed"
        fi
    fi

    if command -v ssh >/dev/null 2>&1; then
        while IFS= read -r line; do
            case "$line" in
                user\ *)
                    if [[ -z "$explicit_user" ]]; then
                        resolved_user="${line#user }"
                    fi
                    ;;
                hostname\ *) resolved_host="${line#hostname }" ;;
                port\ *) resolved_port="${line#port }" ;;
            esac
        done < <(ssh -G "$lookup_seed" 2>/dev/null || true)
    fi

    printf '%s\t%s\t%s\t%s\n' "$lookup_seed" "$resolved_user" "$resolved_host" "$resolved_port"
}

probe_seed() {
    local seed="$1"
    local user="$2"
    local host="$3"
    local port="$4"
    local target output remote_name remote_os remote_home

    target="$seed"
    if [[ -n "$user" ]]; then
        target="${user}@${seed}"
    fi

    output="$(
        ssh \
            -o BatchMode=yes \
            -o ConnectTimeout=2 \
            -o StrictHostKeyChecking=no \
            -o UserKnownHostsFile=/dev/null \
            -p "$port" \
            "$target" \
            'printf "__constant__\t%s\t%s\t%s\n" "$(hostname -s 2>/dev/null || hostname || printf unknown)" "$(uname -s 2>/dev/null || printf unknown)" "$HOME"' \
            2>&1
    )" && {
        remote_name="$(printf '%s\n' "$output" | awk -F'\t' '/^__constant__/ {print $2; exit}')"
        remote_os="$(printf '%s\n' "$output" | awk -F'\t' '/^__constant__/ {print $3; exit}')"
        remote_home="$(printf '%s\n' "$output" | awk -F'\t' '/^__constant__/ {print $4; exit}')"
        printf 'yes\t%s\t%s\t%s\t\n' "${remote_name:-$seed}" "${remote_os:-unknown}" "${remote_home:-unknown}"
        return 0
    }

    output="$(printf '%s' "$output" | tr '\n' ' ' | sed -E 's/[[:space:]]+/ /g; s/^ //; s/ $//')"
    printf 'no\t-\t-\t-\t%s\n' "$output"
    return 1
}

render_scan_json() {
    local candidates_file="$1"
    python3 - "$candidates_file" <<'PY'
import json
import sys
from pathlib import Path

path = Path(sys.argv[1])
candidates = []
for raw in path.read_text(encoding="utf-8").splitlines():
    if not raw.strip():
        continue
    seed, user, host, port, reachable, remote_name, remote_os, remote_home, error = raw.split("\t", 8)
    candidates.append(
        {
            "seed": seed,
            "user": user,
            "host": host,
            "port": int(port),
            "reachable": reachable == "yes",
            "remote_name": remote_name,
            "remote_os": remote_os,
            "remote_home": remote_home,
            "error": error,
        }
    )
print(json.dumps({"candidates": candidates}, indent=2))
PY
}

print_scan_table() {
    local candidates_file="$1"
    printf 'reach\tseed\tuser\thost\tos\thome\n'
    while IFS=$'\t' read -r seed user host port reachable remote_name remote_os remote_home error; do
        [[ -n "$seed" ]] || continue
        printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$reachable" "$seed" "$user" "$host" "$remote_os" "$remote_home"
        if [[ "$reachable" != "yes" && -n "$error" ]]; then
            printf '  error: %s\n' "$error"
        fi
    done <"$candidates_file"
}

discover_candidates() {
    local explicit_hosts_file="$1"
    local default_user="$2"
    local use_ssh_config="$3"
    local use_known_hosts="$4"
    local use_arp="$5"
    local seeds_file unique_seeds_file candidates_file

    seeds_file="$(mktemp)"
    unique_seeds_file="$(mktemp)"
    candidates_file="$(mktemp)"

    cat "$explicit_hosts_file" >>"$seeds_file"
    if [[ "$use_ssh_config" == "true" ]]; then
        scan_ssh_config "$seeds_file"
    fi
    if [[ "$use_known_hosts" == "true" ]]; then
        scan_known_hosts "$seeds_file"
    fi
    if [[ "$use_arp" == "true" ]]; then
        scan_arp_neighbors "$seeds_file"
    fi

    sed -i.bak '/^[[:space:]]*$/d' "$seeds_file" 2>/dev/null || sed -i '' '/^[[:space:]]*$/d' "$seeds_file" 2>/dev/null || true
    unique_lines "$seeds_file" "$unique_seeds_file"

    while IFS= read -r seed; do
        [[ -n "$seed" ]] || continue
        local probe_output
        IFS=$'\t' read -r resolved_seed resolved_user resolved_host resolved_port <<<"$(resolve_seed "$seed" "$default_user")"
        probe_output="$(probe_seed "$resolved_seed" "$resolved_user" "$resolved_host" "$resolved_port" || true)"
        IFS=$'\t' read -r reachable remote_name remote_os remote_home error <<<"$probe_output"
        printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
            "$resolved_seed" \
            "$resolved_user" \
            "$resolved_host" \
            "$resolved_port" \
            "$reachable" \
            "$remote_name" \
            "$remote_os" \
            "$remote_home" \
            "$error" >>"$candidates_file"
    done <"$unique_seeds_file"

    unique_lines "$candidates_file" "${candidates_file}.uniq"
    mv "${candidates_file}.uniq" "$candidates_file"

    rm -f "$seeds_file" "$unique_seeds_file" "$seeds_file.bak"
    printf '%s\n' "$candidates_file"
}

selected_defaults_file() {
    mktemp
}

select_candidates() {
    local candidates_file="$1"
    local select_all="$2"
    local output_file
    local interactive=true
    local line id display chosen_ids chosen_line

    output_file="$(selected_defaults_file)"

    if [[ ! -t 0 || ! -t 1 ]]; then
        interactive=false
    fi

    while IFS=$'\t' read -r seed user host port reachable remote_name remote_os remote_home error; do
        [[ "$reachable" == "yes" ]] || continue
        printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
            "$seed" "$user" "$host" "$port" "$remote_name" "$remote_os" "$remote_home" "$(sanitize_label "${remote_name:-$seed}")" >>"$output_file"
    done <"$candidates_file"

    if [[ ! -s "$output_file" ]]; then
        rm -f "$output_file"
        fail "No reachable SSH targets were discovered."
    fi

    if [[ "$select_all" == "true" || "$interactive" != "true" ]]; then
        printf '%s\n' "$output_file"
        return 0
    fi

    local indexed_file chosen_file
    indexed_file="$(mktemp)"
    chosen_file="$(mktemp)"
    local index=1

    while IFS=$'\t' read -r seed user host port remote_name remote_os remote_home label; do
        printf '%02d\t[%02d] %s (%s@%s, %s)\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
            "$index" "$index" "$label" "$user" "$seed" "$remote_os" "$seed" "$user" "$host" "$port" "$remote_name" "$remote_os" "$remote_home" "$label" >>"$indexed_file"
        index=$((index + 1))
    done <"$output_file"

    if use_gum; then
        local options selected_text
        options="$(awk -F'\t' '{print $2}' "$indexed_file")"
        selected_text="$(printf '%s\n' "$options" | gum choose --no-limit --header 'Select machines to deploy Constant on')"
        if [[ -z "$selected_text" ]]; then
            rm -f "$output_file" "$indexed_file" "$chosen_file"
            fail "No machines selected."
        fi
        while IFS= read -r chosen_line; do
            [[ -n "$chosen_line" ]] || continue
            id="$(printf '%s' "$chosen_line" | sed -E 's/^\[([0-9]+)\].*/\1/')"
            awk -F'\t' -v want="$id" '$1 == want {print $3 "\t" $4 "\t" $5 "\t" $6 "\t" $7 "\t" $8 "\t" $9 "\t" $10}' "$indexed_file" >>"$chosen_file"
        done <<<"$selected_text"
    else
        printf 'Reachable machines:\n'
        awk -F'\t' '{print $2}' "$indexed_file"
        printf 'Select machine numbers (comma-separated or "all"): '
        read -r chosen_ids
        if [[ "$chosen_ids" == "all" || "$chosen_ids" == "ALL" ]]; then
            awk -F'\t' '{print $3 "\t" $4 "\t" $5 "\t" $6 "\t" $7 "\t" $8 "\t" $9 "\t" $10}' "$indexed_file" >"$chosen_file"
        else
            chosen_ids="$(printf '%s' "$chosen_ids" | tr ',' ' ')"
            for id in $chosen_ids; do
                id="$(printf '%s' "$id" | sed -E 's/^0*([0-9]+)$/\1/')"
                [[ -n "$id" ]] || continue
                awk -F'\t' -v want="$id" '$1 == want {print $3 "\t" $4 "\t" $5 "\t" $6 "\t" $7 "\t" $8 "\t" $9 "\t" $10}' "$indexed_file" >>"$chosen_file"
            done
        fi
    fi

    if [[ ! -s "$chosen_file" ]]; then
        rm -f "$output_file" "$indexed_file" "$chosen_file"
        fail "No machines selected."
    fi

    rm -f "$output_file" "$indexed_file"
    printf '%s\n' "$chosen_file"
}

prompt_value() {
    local prompt="$1"
    local default_value="$2"
    local value

    if use_gum; then
        value="$(gum input --prompt "$prompt " --value "$default_value")"
    else
        printf '%s [%s]: ' "$prompt" "$default_value" >&2
        read -r value
    fi

    if [[ -z "$value" ]]; then
        value="$default_value"
    fi

    printf '%s\n' "$value"
}

finalize_selection() {
    local selected_file="$1"
    local local_label="$2"
    local default_user="$3"
    local non_interactive="$4"
    local output_file

    output_file="$(mktemp)"

    printf '%s\tlocal\tlocal\t%s\n' "$local_label" "$default_user" >>"$output_file"

    while IFS=$'\t' read -r seed user host port remote_name remote_os remote_home label; do
        [[ -n "$seed" ]] || continue
        local machine_label ssh_user
        machine_label="$label"
        ssh_user="${user:-$default_user}"
        if [[ -z "$ssh_user" ]]; then
            ssh_user="$(id -un)"
        fi

        if [[ "$non_interactive" != "true" ]]; then
            machine_label="$(prompt_value "Label for ${seed}" "$machine_label")"
            ssh_user="$(prompt_value "SSH user for ${seed}" "$ssh_user")"
        fi

        machine_label="$(sanitize_label "$machine_label")"
        [[ -n "$machine_label" ]] || machine_label="$(sanitize_label "$seed")"
        printf '%s\tremote\t%s\t%s\n' "$machine_label" "$ssh_user" "$seed" >>"$output_file"
    done <"$selected_file"

    printf '%s\n' "$output_file"
}

write_fleet_config() {
    local finalized_file="$1"
    local output_path="$2"
    local repo_dir="$3"

    python3 - "$finalized_file" "$output_path" "$repo_dir" <<'PY'
import json
import sys
from pathlib import Path

finalized = Path(sys.argv[1])
output = Path(sys.argv[2]).expanduser()
repo_dir = sys.argv[3]

machines = []
local_machine = None
for raw in finalized.read_text(encoding="utf-8").splitlines():
    if not raw.strip():
        continue
    label, role, user, seed = raw.split("\t", 3)
    if role == "local":
        local_machine = label
        machines.append(
            {
                "label": label,
                "target": "local",
                "auto_clis": ["codex", "vibe", "claude"],
                "manual_clis": ["copilot"],
                "backends": ["omc", "cli-local", "zellij"],
            }
        )
    else:
        machines.append(
            {
                "label": label,
                "target": f"{user}@{seed}",
                "auto_clis": ["codex", "vibe", "claude"],
                "manual_clis": ["copilot"],
                "backends": ["cli-ssh", "zellij"],
            }
        )

payload = {
    "version": 1,
    "local_machine": local_machine or "command-center",
    "repo_dir": repo_dir,
    "machines": machines,
}

output.parent.mkdir(parents=True, exist_ok=True)
output.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(str(output))
PY
}

run_install() {
    local repo_dir="$1"
    "$script_dir/constant-fleet-install.sh" install --yes --repo-dir "$repo_dir"
}

mode="configure"
json_output=false
yes=false
install_after=false
all_reachable=false
use_ssh_config=true
use_known_hosts=true
use_arp=true
default_user=""
repo_dir="$(zellij_ai_default_repo_dir)"
output_path="$(zellij_ai_config_root)/fleet.json"
local_label="$(sanitize_label "$(current_local_label)")"
explicit_hosts_file="$(mktemp)"

if [[ $# -gt 0 ]]; then
    case "$1" in
        scan|configure|deploy)
            mode="$1"
            shift
            ;;
        -h|--help|help)
            usage
            rm -f "$explicit_hosts_file"
            exit 0
            ;;
    esac
fi

while [[ $# -gt 0 ]]; do
    case "$1" in
        --host)
            printf '%s\n' "$2" >>"$explicit_hosts_file"
            shift 2
            ;;
        --user)
            default_user="$2"
            shift 2
            ;;
        --repo-dir)
            repo_dir="$2"
            shift 2
            ;;
        --local-label)
            local_label="$(sanitize_label "$2")"
            shift 2
            ;;
        --output)
            output_path="$2"
            shift 2
            ;;
        --json)
            json_output=true
            shift
            ;;
        --yes)
            yes=true
            shift
            ;;
        --all-reachable)
            all_reachable=true
            shift
            ;;
        --no-ssh-config)
            use_ssh_config=false
            shift
            ;;
        --no-known-hosts)
            use_known_hosts=false
            shift
            ;;
        --no-arp)
            use_arp=false
            shift
            ;;
        --install)
            install_after=true
            shift
            ;;
        -h|--help)
            usage
            rm -f "$explicit_hosts_file"
            exit 0
            ;;
        *)
            rm -f "$explicit_hosts_file"
            echo "Unknown option: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

if [[ "$mode" == "deploy" ]]; then
    install_after=true
fi

if [[ -z "$default_user" ]]; then
    default_user="$(id -un)"
fi

zellij_ai_require_command ssh
zellij_ai_require_command python3

candidates_file="$(discover_candidates "$explicit_hosts_file" "$default_user" "$use_ssh_config" "$use_known_hosts" "$use_arp")"
rm -f "$explicit_hosts_file"

if [[ "$mode" == "scan" ]]; then
    if $json_output; then
        render_scan_json "$candidates_file"
    else
        print_scan_table "$candidates_file"
    fi
    rm -f "$candidates_file"
    exit 0
fi

selected_file="$(select_candidates "$candidates_file" "$([[ "$yes" == "true" || "$all_reachable" == "true" ]] && printf true || printf false)")"
finalized_file="$(finalize_selection "$selected_file" "$local_label" "$default_user" "$([[ "$yes" == "true" ]] && printf true || printf false)")"
config_written="$(write_fleet_config "$finalized_file" "$output_path" "$repo_dir")"

log "Wrote fleet config to ${config_written}"

if [[ "$install_after" == "true" ]]; then
    run_install "$repo_dir"
else
    log "Run scripts/constant-fleet-install.sh install --yes to deploy now."
fi

rm -f "$candidates_file" "$selected_file" "$finalized_file"
