#!/usr/bin/env bash
# ADR-0003: core crates stay synchronous and UI-agnostic. They must not depend,
# even transitively, on tauri, axum or tokio.
set -euo pipefail

status=0
for crate in yaoplm-core yaoplm-formats yaoplm-hash yaoplm-device yaoplm-i18n; do
  deps=$(cargo tree -p "$crate" --all-features --target all --edges normal,build \
    --prefix none --format '{p}' | awk '{print $1}' | sort -u)
  if offenders=$(grep -E '^(tauri|axum|tokio)(-|$)' <<<"$deps"); then
    echo "::error::$crate depends on: $(tr '\n' ' ' <<<"$offenders")(ADR-0003)"
    status=1
  else
    echo "$crate: ok"
  fi
done
exit "$status"
