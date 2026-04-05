#!/usr/bin/env bash
set -euo pipefail

script_source="${BASH_SOURCE[0]:-$0}"
while [[ -L "$script_source" ]]; do
    script_dir="$(cd "$(dirname "$script_source")" && pwd -P)"
    script_source="$(readlink "$script_source")"
    [[ "$script_source" != /* ]] && script_source="$script_dir/$script_source"
done
script_dir="$(cd "$(dirname "$script_source")" && pwd -P)"
if [[ -f "$script_dir/zellij-ai-common.sh" ]]; then
    source "$script_dir/zellij-ai-common.sh"
else
    zellij_ai_default_repo_dir() {
        printf '%s\n' '$HOME/constant'
    }

    zellij_ai_default_codex_home() {
        printf '%s\n' '$HOME/.codex-profiles/codex'
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

    zellij_ai_current_machine_name() {
        if [[ -n "${ZELLIJ_AI_MACHINE_NAME:-}" ]]; then
            printf '%s\n' "${ZELLIJ_AI_MACHINE_NAME}"
        else
            hostname -s 2>/dev/null || hostname || printf 'local'
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
fi

usage() {
    cat <<EOF
Usage: $(basename "$0") <check|install> [options]

Options:
  --repo-dir DIR       Repository path on this machine
                       default: $(zellij_ai_default_repo_dir)
  --codex-image IMAGE  Deprecated, ignored
  --yes                Required for non-interactive install mode
  -h, --help           Show this help
EOF
}

log() {
    printf '[install] %s\n' "$*"
}

fail() {
    printf '[install] %s\n' "$*" >&2
    exit 1
}

confirm_install() {
    if $yes; then
        return 0
    fi

    if [[ ! -t 0 ]]; then
        fail "Install mode requires --yes when stdin is not interactive."
    fi

    printf 'Proceed with installation changes on this machine? [y/N] '
    read -r answer
    [[ "$answer" == "y" || "$answer" == "Y" ]]
}

need_sudo() {
    [[ "${EUID:-$(id -u)}" -ne 0 ]]
}

run_root() {
    if ! need_sudo; then
        "$@"
        return 0
    fi

    if command -v sudo >/dev/null 2>&1; then
        sudo "$@"
        return 0
    fi

    fail "sudo is required to run: $*"
}

ensure_command() {
    command -v "$1" >/dev/null 2>&1
}

warn_deprecated() {
    printf '[install] %s\n' "$*" >&2
}

refresh_path() {
    export PATH="$(zellij_ai_agent_path)"

    if [[ -x /opt/homebrew/bin/brew ]]; then
        eval "$(/opt/homebrew/bin/brew shellenv)"
    elif [[ -x /usr/local/bin/brew ]]; then
        eval "$(/usr/local/bin/brew shellenv)"
    fi
}

ensure_homebrew() {
    if command -v brew >/dev/null 2>&1; then
        return 0
    fi

    ensure_command curl || fail "curl is required to install Homebrew."
    NONINTERACTIVE=1 /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
    refresh_path
    ensure_command brew || fail "Homebrew installation did not make brew available."
}

ensure_uv() {
    if command -v uv >/dev/null 2>&1; then
        return 0
    fi

    case "$os_name" in
        Darwin)
            ensure_homebrew
            brew install uv
            ;;
        Linux)
            ensure_command curl || fail "curl is required to install uv."
            curl -LsSf https://astral.sh/uv/install.sh | sh
            ;;
        *)
            fail "Unsupported OS for uv installation: $os_name"
            ;;
    esac

    refresh_path
    ensure_command uv || fail "uv installation did not make uv available."
}

ensure_npm_global() {
    local package_name="$1"
    local binary_name="$2"

    if command -v "$binary_name" >/dev/null 2>&1; then
        return 0
    fi

    mkdir -p "$HOME/.npm-global"
    export PATH="$(zellij_ai_agent_path)"

    if npm install -g --prefix "$HOME/.npm-global" "$package_name"; then
        return 0
    fi

    fail "Failed to install ${package_name} with npm."
}

ensure_uv_tool() {
    local package_name="$1"
    local binary_name="$2"

    if command -v "$binary_name" >/dev/null 2>&1; then
        return 0
    fi

    ensure_uv
    uv tool install "$package_name"
    refresh_path
    ensure_command "$binary_name" || fail "Failed to install ${package_name} with uv."
}

os_name="$(uname -s)"
linux_id=""
repo_dir="$(zellij_ai_expand_home_path "$(zellij_ai_default_repo_dir)")"
codex_home="$(zellij_ai_expand_home_path "$(zellij_ai_default_codex_home)")"
yes=false

refresh_path

if [[ -f /etc/os-release ]]; then
    # shellcheck disable=SC1091
    source /etc/os-release
    linux_id="${ID:-}"
fi

if [[ $# -lt 1 ]]; then
    usage >&2
    exit 2
fi

mode="$1"

case "$mode" in
    -h|--help|help)
        usage
        exit 0
        ;;
esac

shift

while [[ $# -gt 0 ]]; do
    case "$1" in
        --repo-dir)
            repo_dir="$(zellij_ai_expand_home_path "$2")"
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

print_status() {
    local key="$1"
    local value="$2"
    printf '%s=%s\n' "$key" "$value"
}

run_check() {
    local ok=true
    local repo_state="missing"
    local codex_home_state="missing"
    local triple_help_state="missing"
    local binary

    refresh_path

    print_status machine "$(zellij_ai_current_machine_name)"
    print_status os "$os_name"
    [[ -n "$linux_id" ]] && print_status linux_id "$linux_id"
    print_status repo_dir "$repo_dir"
    print_status codex_home "$codex_home"

    for binary in bash git zellij node npm uv claude codex copilot vibe; do
        if command -v "$binary" >/dev/null 2>&1; then
            print_status "$binary" "$(command -v "$binary")"
        else
            print_status "$binary" "missing"
            ok=false
        fi
    done

    if [[ -x "$repo_dir/scripts/constant-machine.sh" || -x "$repo_dir/scripts/zellij-ai-triple.sh" ]]; then
        repo_state="present"
    else
        ok=false
    fi
    print_status repo "$repo_state"

    if [[ -d "$codex_home" ]]; then
        codex_home_state="present"
    else
        ok=false
    fi
    print_status codex_profile "$codex_home_state"

    if [[ -x "$repo_dir/scripts/constant-machine.sh" ]] && "$repo_dir/scripts/constant-machine.sh" --help >/dev/null 2>&1; then
        triple_help_state="ok"
    elif [[ -x "$repo_dir/scripts/zellij-ai-triple.sh" ]] && "$repo_dir/scripts/zellij-ai-triple.sh" --help >/dev/null 2>&1; then
        triple_help_state="ok"
    else
        ok=false
    fi
    print_status triple_help "$triple_help_state"

    $ok
}

install_darwin() {
    ensure_homebrew
    brew install git zellij node uv
}

install_linux_ubuntu() {
    run_root apt-get update
    run_root apt-get install -y git curl

    if ! command -v node >/dev/null 2>&1; then
        run_root apt-get install -y nodejs
    fi

    if ! command -v npm >/dev/null 2>&1; then
        run_root apt-get install -y npm
    fi

    if ! command -v zellij >/dev/null 2>&1; then
        if command -v snap >/dev/null 2>&1; then
            run_root snap install zellij --classic
        else
            run_root apt-get install -y zellij
        fi
    fi
}

install_linux_photon() {
    local packages=()

    if ! command -v git >/dev/null 2>&1; then
        packages+=(git)
    fi
    if ! command -v curl >/dev/null 2>&1; then
        packages+=(curl)
    fi
    if ! command -v node >/dev/null 2>&1; then
        packages+=(nodejs)
    fi
    if ! command -v npm >/dev/null 2>&1; then
        packages+=(npm)
    fi

    if [[ ${#packages[@]} -gt 0 ]]; then
        run_root tdnf install -y "${packages[@]}"
    fi

    if ! command -v zellij >/dev/null 2>&1; then
        fail "zellij is missing on Photon OS and no native install path is scripted here."
    fi
}

run_install() {
    confirm_install || fail "Installation cancelled."

    mkdir -p "$repo_dir"

    case "$os_name" in
        Darwin)
            install_darwin
            ;;
        Linux)
            case "$linux_id" in
                ubuntu|debian)
                    install_linux_ubuntu
                    ;;
                photon)
                    install_linux_photon
                    ;;
                *)
                    fail "Unsupported Linux distribution: ${linux_id:-unknown}"
                    ;;
            esac
            ;;
        *)
            fail "Unsupported OS: $os_name"
            ;;
    esac

    refresh_path
    mkdir -p "$codex_home"
    ensure_npm_global "@anthropic-ai/claude-code" claude
    ensure_npm_global "@openai/codex" codex
    ensure_npm_global "@github/copilot" copilot
    ensure_uv_tool "mistral-vibe" vibe

    run_check
}

case "$mode" in
    check)
        run_check
        ;;
    install)
        run_install
        ;;
    *)
        echo "Unknown mode: $mode" >&2
        usage >&2
        exit 2
        ;;
esac
