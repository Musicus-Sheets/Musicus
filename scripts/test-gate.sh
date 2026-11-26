#!/usr/bin/env bash
# The merge gate of workflow `Tests` (job `tests`, a-test-report): fails unless every expected
# result is there and no test failed or was flaky.
#
# Usage: scripts/test-gate.sh <results-dir> <report.json>
#   <results-dir>  the downloaded artifacts: results-unit/ (nextest JUnit files, doctests-status)
#                  and results-e2e-<test>/e2e-<test>-junit.xml for every test in $E2E_REQUIRED
#   E2E_REQUIRED   the browser tests that must have results (default: smoke german pseudo outage)
set -euo pipefail

results="${1:?usage: scripts/test-gate.sh <results-dir> <report.json>}"
report="${2:?usage: scripts/test-gate.sh <results-dir> <report.json>}"
required="${E2E_REQUIRED:-smoke german pseudo outage}"
status=0
error() { echo "::error::$1"; status=1; }

# A missing file means a build or a job failed before it could report (see that job).
for f in nextest-ci-junit.xml nextest-ci-embed-junit.xml; do
  [[ -f "$results/results-unit/$f" ]] || error "No results of the unit tests ($f): see the job unit."
done
doc="$(cat "$results/results-unit/doctests-status" 2>/dev/null || echo missing)"
[[ "$doc" == 0 ]] || error "The doctests failed or did not run (status $doc): see the job unit."
for t in $required; do
  [[ -f "$results/results-e2e-$t/e2e-$t-junit.xml" ]] ||
    error "No results of the browser test $t: see the job e2e ($t) (or build)."
done

if [[ ! -f "$report" ]]; then
  error "No test report: see the step Test report."
else
  read -r failed flaky < <(python3 -c '
import json, sys
c = json.load(open(sys.argv[1]))["counts"]
print(c.get("failed", 0), c.get("flaky", 0))' "$report")
  [[ "$failed" -eq 0 ]] || error "$failed test(s) failed: see the report in the job summary."
  [[ "$flaky" -eq 0 ]] || error "$flaky flaky test(s): failed first, passed on a retry; see the report in the job summary."
fi

[[ $status -ne 0 ]] || echo "All tests passed."
exit "$status"
