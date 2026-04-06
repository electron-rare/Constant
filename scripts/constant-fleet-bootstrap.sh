#!/usr/bin/env bash
set -euo pipefail

tabs_file="${ZELLIJ_AI_FLEET_TABS_FILE:-}"
tabs_created_file="${ZELLIJ_AI_FLEET_CREATED_FILE:-}"
current_label="${ZELLIJ_AI_FLEET_CURRENT_LABEL:-}"
current_wrapper="${ZELLIJ_AI_FLEET_CURRENT_WRAPPER:-}"

if [[ -z "$current_wrapper" || ! -x "$current_wrapper" ]]; then
    echo "Fleet bootstrap wrapper not found: $current_wrapper" >&2
    exit 1
fi

if [[ -n "$current_label" ]]; then
    zellij action rename-tab "$current_label"
fi

if [[ -n "$tabs_created_file" ]]; then
    mkdir -p "$(dirname "$tabs_created_file")"
fi

if [[ -n "$tabs_file" && -f "$tabs_file" && -n "$tabs_created_file" && ! -f "$tabs_created_file" ]]; then
    : >"$tabs_created_file"

    while IFS=$'\t' read -r tab_label layout_file tab_cwd; do
        if [[ -z "$tab_label" || -z "$layout_file" ]]; then
            continue
        fi

        if [[ -n "$tab_cwd" ]]; then
            zellij action new-tab -n "$tab_label" -l "$layout_file" --cwd "$tab_cwd"
        else
            zellij action new-tab -n "$tab_label" -l "$layout_file"
        fi
    done <"$tabs_file"

    if [[ -n "$current_label" ]]; then
        zellij action go-to-tab-name "$current_label"
    fi
fi

exec "$current_wrapper"
