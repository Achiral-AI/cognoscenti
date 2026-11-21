#!/usr/bin/env bash
# One arm of the use-learning study (docs/preregistration-use-learning-2026-11.md).
# usage: KEYFILE=<file, one key per line, 5 fresh orgs> run-use-learning-arm.sh <arm> <stream.jsonl> <out-prefix> [extra flags e.g. --resume]
# arm: off | shipped | shipped-nofb | b2 | bm25 | usecount. Keys are read from KEYFILE and never printed.
# Needs COGNOSCENTI_ACHIRAL_BASE_URL set for the Achiral arms; the token needs memory:control for feedback.
set -eu
arm="$1"; stream="$2"; out="$3"; shift 3
common=(--replay-stream "$stream" --top-k 3 --replicates 5 --seed 0
        --results-json "$out.results.json" --replicate-log "$out.replicates.log" --label "$arm")
unset COGNOSCENTI_ACHIRAL_RETRIEVAL_MODE COGNOSCENTI_ACHIRAL_RETRIEVAL_PARAMS COGNOSCENTI_ACHIRAL_USE_FEEDBACK
case "$arm" in
  bm25|usecount) exec cognoscenti "${common[@]}" --systems "$arm" "$@" ;;
  off)          export COGNOSCENTI_ACHIRAL_RETRIEVAL_PARAMS='{"activationWeightScale":0}' COGNOSCENTI_ACHIRAL_USE_FEEDBACK=1 ;;
  shipped)      export COGNOSCENTI_ACHIRAL_USE_FEEDBACK=1 ;;
  shipped-nofb) ;;
  b2)           export COGNOSCENTI_ACHIRAL_RETRIEVAL_PARAMS='{"activationRangeWeight":1.0}' COGNOSCENTI_ACHIRAL_USE_FEEDBACK=1 ;;
  *) echo "unknown arm: $arm" >&2; exit 2 ;;
esac
export COGNOSCENTI_ACHIRAL_API_KEYS="$(paste -sd, "$KEYFILE")"
export COGNOSCENTI_ACHIRAL_SETTLE_MS="${COGNOSCENTI_ACHIRAL_SETTLE_MS:-2000}"
export COGNOSCENTI_ACHIRAL_CONFIRM_MS="${COGNOSCENTI_ACHIRAL_CONFIRM_MS:-600000}"
exec cognoscenti "${common[@]}" --systems achiral "$@"
