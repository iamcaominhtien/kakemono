#!/usr/bin/env bash
# Downloads the web pages listed in tests/data/web-pages.tsv that are not in
# tests/data/web yet (tests/web_pages.rs reads them; they are not committed).
# A page changes over time: delete its file to download it again.
set -euo pipefail
cd "$(dirname "$0")/../tests/data"
mkdir -p web
while IFS=$'\t' read -r name url; do
  case "$name" in ''|'#'*) continue ;; esac
  file="web/$name.html"
  [[ -f "$file" ]] && continue
  echo "downloading $name"
  if ! curl -fsSL --proto =https --proto-redir =https --retry 3 --retry-delay 5 \
      -A "kakemono-tests (+https://github.com/iamcaominhtien/kakemono)" -o "$file.part" "$url"; then
    echo "could not download $url" >&2
    rm -f "$file.part"
    continue
  fi
  mv "$file.part" "$file"
  sleep 1
done < web-pages.tsv
echo "test pages ready"
