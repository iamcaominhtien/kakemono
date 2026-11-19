#!/usr/bin/env bash
# Downloads the Word documents of tests/data/word-corpus.tsv, with the PDF Word
# made of each, into tests/data/word-corpus/ (not committed), and checks
# their sha256. A file whose source changed is skipped with a warning.
# Usage: scripts/fetch-word-corpus.sh [JOBS]
set -euo pipefail
cd "$(dirname "$0")/../tests/data"
mkdir -p word-corpus
jobs="${1:-8}"
fetch() {
  local file="$1" sum="$2" url="$3"
  if [[ -f "word-corpus/$file" ]] && echo "$sum  word-corpus/$file" | sha256sum -c --status; then
    return 0
  fi
  if ! curl -fsSL --proto =https --proto-redir =https --retry 3 --retry-delay 5 -A "kakemono-tests" \
      -o "word-corpus/$file.part" "$url"; then
    echo "warning: cannot download $file" >&2
    rm -f "word-corpus/$file.part"
    return 0
  fi
  if echo "$sum  word-corpus/$file.part" | sha256sum -c --status; then
    mv "word-corpus/$file.part" "word-corpus/$file"
  else
    echo "warning: $file changed at its source, skipped" >&2
    rm -f "word-corpus/$file.part"
  fi
}
export -f fetch
grep -v '^#' word-corpus.tsv | while IFS=$'\t' read -r id ext word_sum word_url pdf_sum pdf_url; do
  [[ -n "$id" ]] || continue
  printf '%s\0%s\0%s\0' "$id.$ext" "$word_sum" "$word_url"
  printf '%s\0%s\0%s\0' "$id.pdf" "$pdf_sum" "$pdf_url"
done | xargs -0 -n 3 -P "$jobs" bash -c 'fetch "$@"' _
echo "word corpus ready: $(ls word-corpus | grep -vc '\.pdf$') documents"
