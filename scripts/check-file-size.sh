#!/usr/bin/env bash
# Fail when a tracked source file (Rust, Python, shell, Swift) exceeds MAX_LINES.
# There is no allowlist: split an oversized file by responsibility instead.

set -euo pipefail

MAX_LINES=1000

cd "$(git rev-parse --show-toplevel)"

# Passes on the NUL-separated paths that exist: a path deleted in the working
# tree is still listed by git until staged. Builtins only, so no process per file.
existing() {
  local file
  while IFS= read -r -d '' file; do
    if [[ -f "$file" ]]; then
      printf '%s\0' "$file"
    fi
  done
}

# One wc per batch rather than per file: starting processes per file takes
# minutes under Git Bash on Windows. Counted before the loop so a file wc
# cannot read fails the check instead of being skipped.
if ! counts=$(git ls-files -z -- '*.rs' '*.py' '*.sh' '*.swift' | existing | xargs -0 wc -l); then
  echo "FAIL: could not count lines in every source file" >&2
  exit 1
fi

failures=0
while read -r lines file; do
  # wc ends each batch with a "total" line; no checked file has that name.
  [[ "$file" == total ]] && continue
  if ((lines > MAX_LINES)); then
    echo "FAIL: $file has $lines lines (limit $MAX_LINES)"
    failures=$((failures + 1))
  fi
done <<<"$counts"

if ((failures > 0)); then
  echo
  echo "$failures file(s) over $MAX_LINES lines. Split them into modules by responsibility."
  exit 1
fi

echo "All source files are within $MAX_LINES lines."
