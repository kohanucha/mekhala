#!/bin/bash
# Run cargo-mutants the fast way.
#
#   ./scripts/mutants.sh                  # mutants in lines changed vs main
#   ./scripts/mutants.sh --base HEAD~3    # ... against another ref
#   ./scripts/mutants.sh --full           # whole configured scope, parallel
#   ./scripts/mutants.sh --iterate        # skip mutants caught last run
#
# Why these flags:
# - `--in-diff` takes a *diff file*, not a git ref, so the diff is generated
#   here (it includes uncommitted work).
# - `-j` cannot be combined with `--in-place`. Dropping `--in-place` makes
#   cargo-mutants work on its own copy of the tree: safe for parallel jobs and
#   for an interrupted run, at the cost of one baseline build (~20s).

set -euo pipefail

cd "$(dirname "$0")/.."

BASE="main"
if command -v nproc >/dev/null 2>&1; then
    JOBS="$(nproc)"
else
    JOBS="$(sysctl -n hw.ncpu)"
fi
TIMEOUT=60
MODE=diff
ITERATE=()

usage() {
    cat <<'EOF'
Usage: ./scripts/mutants.sh [options]

  --full              Run the whole scope from .cargo/mutants.toml (release run).
  --base <ref>        Diff mode base ref (default: main).
  --iterate           Skip mutants caught or unviable in the previous run.
  -j, --jobs <n>      Parallel jobs (default: all CPUs).
  -t, --timeout <s>   Per-mutant timeout in seconds (default: 60).
  -h, --help          Show this help.

Without --full only mutants in lines changed since the merge-base of <ref> and
HEAD are tested, which is ~7x fewer mutants while iterating on a branch.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --full)
            MODE=full
            shift
            ;;
        --base)
            BASE="${2:?--base needs a git ref}"
            shift 2
            ;;
        --iterate)
            ITERATE=(--iterate)
            shift
            ;;
        -j|--jobs)
            JOBS="${2:?--jobs needs a number}"
            shift 2
            ;;
        -t|--timeout)
            TIMEOUT="${2:?--timeout needs a number}"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "unknown argument: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

ARGS=(-j "$JOBS" -t "$TIMEOUT")
if [[ ${#ITERATE[@]} -gt 0 ]]; then
    ARGS+=("${ITERATE[@]}")
fi

if [[ "$MODE" == full ]]; then
    echo "Testing the full configured scope with $JOBS job(s)..."
    cargo mutants "${ARGS[@]}"
    exit $?
fi

if ! MERGE_BASE="$(git merge-base "$BASE" HEAD 2>/dev/null)"; then
    echo "cannot resolve base ref: $BASE" >&2
    exit 1
fi

DIFF_FILE="$(mktemp "${TMPDIR:-/tmp}/mekhala-mutants.XXXXXX")"
trap 'rm -f "$DIFF_FILE"' EXIT

git diff "$MERGE_BASE" > "$DIFF_FILE"
COUNT="$(cargo mutants --list --in-diff "$DIFF_FILE" 2>/dev/null | wc -l | tr -d ' ')"
if [[ "$COUNT" == "0" ]]; then
    echo "No mutants in the diff vs $BASE ($MERGE_BASE); nothing to do."
    exit 0
fi

echo "Testing $COUNT mutant(s) in code changed vs $BASE ($MERGE_BASE) with $JOBS job(s)..."
cargo mutants "${ARGS[@]}" --in-diff "$DIFF_FILE"
