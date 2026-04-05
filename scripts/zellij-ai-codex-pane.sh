#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"

usage() {
    cat <<'EOF'
Usage: zellij-ai-codex-pane.sh <slot>

slot:
  1   Launch the first Codex container/profile
  2   Launch the second Codex container/profile
EOF
}

find_codex_binary() {
    if [[ -n "${ZELLIJ_AI_CODEX_BINARY:-}" && -x "${ZELLIJ_AI_CODEX_BINARY}" ]]; then
        printf '%s\n' "${ZELLIJ_AI_CODEX_BINARY}"
        return 0
    fi

    if [[ -x "$HOME/.npm-global/lib/node_modules/@openai/codex/node_modules/@openai/codex-linux-x64/vendor/x86_64-unknown-linux-musl/codex/codex" ]]; then
        printf '%s\n' "$HOME/.npm-global/lib/node_modules/@openai/codex/node_modules/@openai/codex-linux-x64/vendor/x86_64-unknown-linux-musl/codex/codex"
        return 0
    fi

    if [[ -n "$(command -v codex || true)" ]]; then
        local wrapper_dir package_dir candidate
        wrapper_dir="$(cd "$(dirname "$(command -v codex)")" && pwd -P)"
        package_dir="$(cd "$wrapper_dir/../lib/node_modules/@openai/codex/node_modules/@openai" 2>/dev/null && pwd -P || true)"
        if [[ -n "$package_dir" ]]; then
            candidate="$(find "$package_dir" -path '*/vendor/*/codex/codex' -type f -print -quit 2>/dev/null || true)"
            if [[ -n "$candidate" && -x "$candidate" ]]; then
                printf '%s\n' "$candidate"
                return 0
            fi
        fi
    fi

    return 1
}

if [[ $# -ne 1 ]]; then
    usage >&2
    exit 2
fi

slot="$1"

case "$slot" in
    1)
        label="${ZELLIJ_AI_CODEX1_LABEL:-codex-1}"
        profile_dir="${ZELLIJ_AI_CODEX1_HOME:-$HOME/.codex-profiles/codex-1}"
        ;;
    2)
        label="${ZELLIJ_AI_CODEX2_LABEL:-codex-2}"
        profile_dir="${ZELLIJ_AI_CODEX2_HOME:-$HOME/.codex-profiles/codex-2}"
        ;;
    *)
        usage >&2
        exit 2
        ;;
esac

workspace="${ZELLIJ_AI_WORKSPACE:-$PWD}"
image="${ZELLIJ_AI_CODEX_IMAGE:-codercom/code-server:latest}"
main_codex_config="${ZELLIJ_AI_MAIN_CODEX_CONFIG:-$HOME/.codex/config.toml}"
codex_binary="$(find_codex_binary || true)"
term_value="${ZELLIJ_AI_CODEX_TERM:-${TERM:-xterm-256color}}"
color_term_value="${COLORTERM:-truecolor}"

if [[ "$term_value" == "dumb" ]]; then
    term_value="xterm-256color"
fi

if [[ ! -d "$workspace" ]]; then
    echo "Workspace not found: $workspace" >&2
    exit 1
fi

if [[ -z "$codex_binary" || ! -x "$codex_binary" ]]; then
    echo "Unable to locate the native Codex binary on the host." >&2
    echo "Set ZELLIJ_AI_CODEX_BINARY if Codex is installed elsewhere." >&2
    exit 1
fi

platform_dir="$(dirname "$(dirname "$codex_binary")")"
rg_binary="$platform_dir/path/rg"

if [[ ! -x "$rg_binary" ]]; then
    echo "Bundled rg binary not found next to Codex: $rg_binary" >&2
    exit 1
fi

if ! docker image inspect "$image" >/dev/null 2>&1; then
    echo "Docker image not found locally: $image" >&2
    echo "Pull or build the image first, or relaunch with --codex-image." >&2
    exit 1
fi

mkdir -p "$profile_dir"

if [[ ! -f "$profile_dir/config.toml" && -f "$main_codex_config" ]]; then
    cp "$main_codex_config" "$profile_dir/config.toml"
fi

container_name="zellij-$(printf '%s' "$label" | tr '[:upper:]' '[:lower:]' | tr -cs '[:alnum:]._-' '-')"
uid="$(id -u)"
gid="$(id -g)"

read -r -d '' inner_script <<EOF || true
set -euo pipefail
cd /workspace
export HOME=/codex-home
export CODEX_HOME=/codex-home
echo "Codex pane: $label"
echo "workspace: /workspace"
echo "profile: /codex-home"
echo "image: $image"
echo
if [ ! -f /codex-home/auth.json ]; then
  echo "No auth.json found for $label."
  echo "Starting: codex login --device-auth"
  echo "Use a different ChatGPT/OpenAI account here than in the other pane."
  echo
  codex login --device-auth
  echo
fi
exec codex --no-alt-screen -C /workspace
EOF

docker_args=(
    run
    --rm
    --init
    -it
    --name "$container_name"
    --hostname "$container_name"
    --user "${uid}:${gid}"
    --workdir /workspace
    --entrypoint bash
    -e HOME=/codex-home
    -e CODEX_HOME=/codex-home
    -e USER="${USER:-$(id -un)}"
    -e TERM="$term_value"
    -e COLORTERM="$color_term_value"
    -e LANG="${LANG:-C.UTF-8}"
    -v "${workspace}:/workspace"
    -v "${profile_dir}:/codex-home"
    -v "${codex_binary}:/usr/local/bin/codex:ro"
    -v "${rg_binary}:/usr/local/bin/rg:ro"
)

if [[ -f "$HOME/.gitconfig" ]]; then
    docker_args+=(-v "$HOME/.gitconfig:/codex-home/.gitconfig:ro")
fi

docker_args+=("$image" -lc "$inner_script")

exec docker "${docker_args[@]}"
