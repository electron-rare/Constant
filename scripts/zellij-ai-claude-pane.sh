#!/usr/bin/env bash
set -euo pipefail

workspace="${ZELLIJ_AI_WORKSPACE:-$PWD}"

if [[ ! -d "$workspace" ]]; then
    echo "Workspace not found: $workspace" >&2
    exit 1
fi

if [[ -n "${ZELLIJ_AI_CLAUDE_CONFIG_DIR:-}" ]]; then
    export CLAUDE_CONFIG_DIR="$ZELLIJ_AI_CLAUDE_CONFIG_DIR"
fi

cd "$workspace"

echo "Claude pane"
echo "workspace: $workspace"
if [[ -n "${CLAUDE_CONFIG_DIR:-}" ]]; then
    echo "config: $CLAUDE_CONFIG_DIR"
fi
echo

exec claude
