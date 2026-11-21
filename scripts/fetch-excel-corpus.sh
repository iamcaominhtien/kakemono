#!/usr/bin/env bash
# Downloads the Excel workbooks of tests/data/excel-corpus.tsv into
# tests/data/excel-corpus/ (not committed), and checks their sha256. A file
# whose source changed is skipped with a warning.
# Usage: scripts/fetch-excel-corpus.sh [JOBS] [NAME...]  (only these files when names are given, e.g. abs-cpi-640101.xlsx)
set -euo pipefail
cd "$(dirname "$0")/../tests/data"
mkdir -p excel-corpus
jobs="${1:-8}"
shift || true
wanted=" $* "
fetch() {
  local file="$1" sum="$2" url="$3"
  if [[ -f "excel-corpus/$file" ]] && echo "$sum  excel-corpus/$file" | sha256sum -c --status; then
    return 0
  fi
  if ! curl -fsSL --proto =https --proto-redir =https --retry 3 --retry-delay 5 -A "kakemono-tests" \
      -o "excel-corpus/$file.part" "$url"; then
    echo "warning: cannot download $file" >&2
    rm -f "excel-corpus/$file.part"
    return 0
  fi
  if echo "$sum  excel-corpus/$file.part" | sha256sum -c --status; then
    mv "excel-corpus/$file.part" "excel-corpus/$file"
  else
    echo "warning: $file changed at its source, skipped" >&2
    rm -f "excel-corpus/$file.part"
  fi
}
export -f fetch
grep -v '^#' excel-corpus.tsv | while IFS=$'\t' read -r file sum url _; do
  [[ -n "$file" ]] || continue
  [[ "$wanted" == "  " || "$wanted" == *" $file "* ]] || continue
  printf '%s\0%s\0%s\0' "$file" "$sum" "$url"
done | xargs -0 -n 3 -P "$jobs" bash -c 'fetch "$@"' _
echo "excel corpus ready: $(find excel-corpus -maxdepth 1 -type f -not -name '*.part' | wc -l) workbooks"
