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
rust_bin="$repo_dir/target/debug/constant"

if [[ ! -x "$rust_bin" ]]; then
    cargo build --manifest-path "$repo_dir/Cargo.toml" --quiet
    if command -v codesign >/dev/null 2>&1; then
        codesign --force -s - "$rust_bin" >/dev/null 2>&1 || true
    fi
fi

exec "$rust_bin" cockpit status-line "$@"
