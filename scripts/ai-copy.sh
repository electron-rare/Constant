#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'EOF'
Usage: ai-copy.sh [options]

Copy text into the macOS clipboard on the current machine.

Options:
  --text TEXT    Copy this text directly
  --file FILE    Copy the contents of FILE
  -h, --help     Show this help

If neither --text nor --file is provided, stdin is copied.
EOF
}

text=""
file=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --text)
            text="$2"
            shift 2
            ;;
        --file)
            file="$2"
            shift 2
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

if [[ -n "$text" && -n "$file" ]]; then
    echo "Use either --text or --file, not both." >&2
    exit 2
fi

if ! command -v pbcopy >/dev/null 2>&1; then
    echo "pbcopy is not available on this machine." >&2
    exit 1
fi

if [[ -n "$text" ]]; then
    printf '%s' "$text" | pbcopy
elif [[ -n "$file" ]]; then
    cat "$file" | pbcopy
elif [[ ! -t 0 ]]; then
    cat | pbcopy
else
    echo "Nothing to copy. Provide --text, --file, or pipe stdin." >&2
    exit 2
fi

printf 'Copied to the macOS clipboard.\n'
