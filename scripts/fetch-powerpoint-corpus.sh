#!/usr/bin/env bash
# Downloads the PowerPoint decks of tests/data/powerpoint-corpus.tsv, and the
# PDF PowerPoint made of each when one is listed, into
# tests/data/powerpoint-corpus/ (not committed) as <id>.pptx and <id>.pdf,
# and checks their sha256. A file whose source changed is skipped with a
# warning. A file stored as a MIME part in base64 (some GitHub repositories)
# is decoded before it is checked.
# Usage: scripts/fetch-powerpoint-corpus.sh [JOBS] [ID...]  (only these decks when ids are given, e.g. uk-0007)
set -euo pipefail
cd "$(dirname "$0")/../tests/data"
mkdir -p powerpoint-corpus
jobs="${1:-8}"
shift || true
wanted=" $* "
fetch() {
  local file="$1" sum="$2" url="$3"
  local path="powerpoint-corpus/$file"
  if [[ -f "$path" ]] && echo "$sum  $path" | sha256sum -c --status; then
    return 0
  fi
  if ! curl -fsSL --proto =https --proto-redir =https --retry 3 --retry-delay 5 -A "kakemono-tests" \
      -o "$path.part" "$url"; then
    echo "warning: cannot download $file" >&2
    rm -f "$path.part"
    return 0
  fi
  if ! echo "$sum  $path.part" | sha256sum -c --status && head -c 64 "$path.part" | grep -q '^Content-Type:'; then
    # A MIME part: the base64 body after the first empty line.
    python3 -I -c '
import base64, sys
data = open(sys.argv[1], "rb").read().replace(b"\r\n", b"\n")
body = data.split(b"\n\n", 1)[1] if b"\n\n" in data else b""
open(sys.argv[1], "wb").write(base64.b64decode(body))
' "$path.part" || true
  fi
  if echo "$sum  $path.part" | sha256sum -c --status; then
    mv "$path.part" "$path"
  else
    echo "warning: $file changed at its source, skipped" >&2
    rm -f "$path.part"
  fi
}
export -f fetch
# Empty columns are kept as "-": `read` would merge neighbouring tabs.
grep -v '^#' powerpoint-corpus.tsv | awk -F'\t' '{for (i = 1; i <= 5; i++) if ($i == "") $i = "-"; print $1, $2, $3, $4, $5}' |
while read -r id sum url pdf_sum pdf_url; do
  [[ -n "$id" ]] || continue
  [[ "$wanted" == "  " || "$wanted" == *" $id "* ]] || continue
  printf '%s\0%s\0%s\0' "$id.pptx" "$sum" "$url"
  if [[ "$pdf_sum" != "-" && "$pdf_url" != "-" ]]; then
    printf '%s\0%s\0%s\0' "$id.pdf" "$pdf_sum" "$pdf_url"
  fi
done | xargs -0 -n 3 -P "$jobs" bash -c 'fetch "$@"' _
echo "powerpoint corpus ready: $(find powerpoint-corpus -maxdepth 1 -name '*.pptx' | wc -l) decks," \
  "$(find powerpoint-corpus -maxdepth 1 -name '*.pdf' | wc -l) PowerPoint PDFs"
