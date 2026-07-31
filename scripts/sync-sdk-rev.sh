#!/usr/bin/env bash
#
# Sync this repo's iota-rust-sdk pin with the rev used by the iota monorepo.
#
# All iota-rust-sdk crates must share one rev, and it must match the rev the
# monorepo pins -- otherwise Cargo builds two incompatible copies of
# iota_sdk_types and monorepo types (test-cluster) stop unifying with ours.
#
#   ./scripts/sync-sdk-rev.sh                  # sync to the monorepo rev pinned here
#   ./scripts/sync-sdk-rev.sh --check          # exit 1 if out of sync (CI)
#   ./scripts/sync-sdk-rev.sh --ref v1.29.0    # follow a tag/branch/sha instead
#   ./scripts/sync-sdk-rev.sh --with-monorepo  # bump the dev-dep rev to develop + sync
#
# Without --ref, the reference point is the monorepo rev this repo already
# pins for its dev-deps (test-cluster), so --check verifies internal
# consistency instead of chasing the develop tip. --with-monorepo defaults
# to develop because it deliberately moves that pin forward.
set -euo pipefail

MONO_REF=""
CHECK_ONLY=0
WITH_MONOREPO=0

while [ $# -gt 0 ]; do
  case "$1" in
    --check)         CHECK_ONLY=1 ;;
    --with-monorepo) WITH_MONOREPO=1 ;;
    --ref)           MONO_REF="${2:?--ref needs a value}"; shift ;;
    -h|--help)       sed -n '2,17p' "$0"; exit 0 ;;
    *)               echo "unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done

die() { echo "error: $*" >&2; exit 1; }

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || true)"
[ -n "$ROOT" ] || die "not inside a git repository"
MANIFEST="$ROOT/Cargo.toml"
[ -f "$MANIFEST" ] || die "no Cargo.toml at $MANIFEST"

if [ -z "$MONO_REF" ]; then
  if [ "$WITH_MONOREPO" -eq 1 ]; then
    MONO_REF="develop"
  else
    PINNED_ALL="$(grep -E 'iotaledger/iota(\.git)?"' "$MANIFEST" \
      | grep -oE 'rev *= *"[0-9a-f]{40}"' | grep -oE '[0-9a-f]{40}' | sort -u)"
    PINNED_N="$(printf '%s\n' "$PINNED_ALL" | grep -c . || true)"
    [ "$PINNED_N" -le 1 ] || die "this repo pins $PINNED_N different monorepo revs; pass --ref"
    if [ "$PINNED_N" -eq 1 ]; then
      MONO_REF="$PINNED_ALL"
    else
      echo "warning: no monorepo rev pinned in $MANIFEST; falling back to develop" >&2
      MONO_REF="develop"
    fi
  fi
fi

RAW="https://raw.githubusercontent.com/iotaledger/iota/${MONO_REF}/Cargo.toml"
MONO_TOML="$(curl -sfL "$RAW")" || die "cannot fetch $RAW"

extract_revs() {
  grep -E 'iotaledger/iota-rust-sdk' \
    | grep -oE 'rev *= *"[0-9a-f]{40}"' \
    | grep -oE '[0-9a-f]{40}' \
    | sort -u
}

WANT_ALL="$(printf '%s\n' "$MONO_TOML" | extract_revs || true)"
WANT_N="$(printf '%s\n' "$WANT_ALL" | grep -c . || true)"
[ "$WANT_N" -gt 0 ] || die "no iota-rust-sdk rev found in $MONO_REF Cargo.toml"
[ "$WANT_N" -eq 1 ] || die "$MONO_REF pins $WANT_N different SDK revs: $(echo $WANT_ALL)"
WANT="$(printf '%s\n' "$WANT_ALL" | head -n1)"

OURS_ALL="$(extract_revs < "$MANIFEST" || true)"
OURS_N="$(printf '%s\n' "$OURS_ALL" | grep -c . || true)"
[ "$OURS_N" -gt 0 ] || die "no iota-rust-sdk rev found in $MANIFEST"
if [ "$OURS_N" -gt 1 ]; then
  echo "warning: this repo pins $OURS_N different SDK revs: $(echo $OURS_ALL)" >&2
  echo "         all of them will be set to $WANT" >&2
fi

URL_LINES=$(grep -cE 'iotaledger/iota-rust-sdk' "$MANIFEST" || true)
REV_LINES=$(grep -E 'iotaledger/iota-rust-sdk' "$MANIFEST" | grep -cE 'rev *= *"' || true)
[ "$URL_LINES" -eq "$REV_LINES" ] \
  || die "$((URL_LINES - REV_LINES)) iota-rust-sdk line(s) have no rev on the same line; fix by hand"

REF_LABEL="$MONO_REF"
printf '%s' "$MONO_REF" | grep -qE '^[0-9a-f]{40}$' && REF_LABEL="${MONO_REF:0:12} (pinned here)"
printf '%-40s : %s\n' "monorepo ${REF_LABEL} pins" "$WANT"
printf '%-40s : %s\n' "this repo pins" "$(echo $OURS_ALL)"

if [ "$OURS_N" -eq 1 ] && [ "$OURS_ALL" = "$WANT" ] && [ "$WITH_MONOREPO" -eq 0 ]; then
  echo "already in sync."
  exit 0
fi

if [ "$CHECK_ONLY" -eq 1 ]; then
  die "out of sync with $MONO_REF -- run ./scripts/sync-sdk-rev.sh"
fi

TMP="$(mktemp)"; trap 'rm -f "$TMP"' EXIT
awk -v want="$WANT" '
  /iotaledger\/iota-rust-sdk/ { gsub(/rev *= *"[0-9a-f]+"/, "rev = \"" want "\"") }
  { print }
' "$MANIFEST" > "$TMP"
cp "$TMP" "$MANIFEST"
echo "updated iota-rust-sdk rev -> $WANT"

if [ "$WITH_MONOREPO" -eq 1 ]; then
  if printf '%s' "$MONO_REF" | grep -qE '^[0-9a-f]{40}$'; then
    MONO_SHA="$MONO_REF"
  else
    MONO_SHA="$(git ls-remote https://github.com/iotaledger/iota "refs/heads/${MONO_REF}" | awk '{print $1}')"
    if [ -z "$MONO_SHA" ]; then
      MONO_SHA="$(git ls-remote https://github.com/iotaledger/iota "refs/tags/${MONO_REF}" | awk '{print $1}')"
    fi
    [ -n "$MONO_SHA" ] || die "cannot resolve iotaledger/iota ref '${MONO_REF}'"
  fi

  if grep -E 'iotaledger/iota(\.git)?"' "$MANIFEST" | grep -q 'tag *= *"'; then
    die "monorepo deps use tag = \"...\"; convert them to rev = \"...\" by hand first"
  fi

  awk -v want="$MONO_SHA" '
    /iotaledger\/iota(\.git)?"/ { gsub(/rev *= *"[0-9a-f]+"/, "rev = \"" want "\"") }
    { print }
  ' "$MANIFEST" > "$TMP"
  cp "$TMP" "$MANIFEST"
  echo "updated monorepo rev      -> $MONO_SHA  (ref: $MONO_REF)"

  # Keep the fastcrypto [patch] pinned to the same version as the monorepo's
  # own [patch."https://github.com/MystenLabs/fastcrypto"] table.
  MONO_FC_VER="$(printf '%s\n' "$MONO_TOML" \
    | awk '/^\[patch\."https:\/\/github\.com\/MystenLabs\/fastcrypto"\]/{s=1; next} s && /^\[/{s=0} s && /^fastcrypto[ =]/{print; exit}' \
    | grep -oE '"=?[0-9][^"]*"' | head -n1 | tr -d '"')"
  if [ -n "$MONO_FC_VER" ] && grep -q '^\[patch\."https://github\.com/MystenLabs/fastcrypto"\]' "$MANIFEST"; then
    awk -v want="$MONO_FC_VER" '
      /^\[patch\."https:\/\/github\.com\/MystenLabs\/fastcrypto"\]/{s=1; print; next}
      s && /^\[/{s=0}
      s && /^fastcrypto *=/{ sub(/"[^"]*"/, "\"" want "\"") }
      { print }
    ' "$MANIFEST" > "$TMP"
    cp "$TMP" "$MANIFEST"
    echo "updated fastcrypto patch  -> $MONO_FC_VER"
  else
    echo "warning: could not sync the fastcrypto patch version (patch table missing on one side)" >&2
  fi
fi

echo
echo "running: cargo update --workspace"
(cd "$ROOT" && cargo update --workspace)

echo
echo "next: cargo check --all-targets"
