#!/usr/bin/env bash
# Full verification:
#   1. Lean checks every proof in proof/KindleCore (and that none is skipped).
#   2. The verified Lean definitions generate test vectors.
#   3. The Rust implementation must reproduce every vector (+ unit tests).
set -euo pipefail
cd "$(dirname "$0")/.."

if grep -RnwE 'sorry|admit|axiom' proof/KindleCore proof/KindleCore.lean; then
  echo "error: unproved statements found in proof/" >&2
  exit 1
fi

echo "==> Lean: checking proofs"
(cd proof && lake build)

echo "==> Lean: generating test vectors"
(cd proof && lake env lean --run Vectors.lean) > tests/vectors.txt.new
if ! cmp -s tests/vectors.txt tests/vectors.txt.new; then
  echo "note: tests/vectors.txt was out of date and has been regenerated" >&2
fi
mv tests/vectors.txt.new tests/vectors.txt

echo "==> Rust: differential tests against the Lean model + unit tests"
cargo test --release

echo "OK: proofs checked, Rust core agrees with the Lean model on $(wc -l < tests/vectors.txt) vectors"
