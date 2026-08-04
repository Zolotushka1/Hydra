#!/bin/sh
# Refuses to let panel or agent runtime state become a tracked file.
#
# Not a second ignore list. The ignore list names directories that exist today,
# and naming one more after each incident is treatment, not prevention: the
# panel's state paths are all configurable through HYDRA_*_PATH, so the next
# script writes state somewhere the list has never heard of. This matches on
# what the files *are*.
#
# It exists because a development script wrote panel state to `.dev-leptos/`,
# which the ignore list did not cover, and a `git add -A` published an Argon2
# admin hash and three live subscription tokens to a public repository.
#
# Matching is on tracked paths rather than the working tree: an ignored file is
# not the problem, a committed one is.
set -eu

fail() {
  printf 'tracked-file check: %s\n' "$1" >&2
  exit 1
}

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

command -v git >/dev/null 2>&1 || fail "git is required"

tracked="$(git ls-files)"
[ -n "$tracked" ] || fail "git ls-files returned nothing, so this check is not working"

# Extensions the panel and the agent write state to. `.ndjson` is every append
# log the panel keeps — audit, operational, user activity, sync history — and
# `.log` is any captured process output.
#
# `.key`, `.pem` and their neighbours are already refused by the ignore list;
# they are repeated here because this check must not depend on that list being
# right.
by_extension='\.(ndjson|log|key|pem|crt|p12|pfx)$'

# Filenames the panel persists state under. These are the defaults; a run with
# custom HYDRA_*_PATH values produces the same content under other names, which
# is what the directory rule below is for.
by_filename='(^|/)(admin|users|nodes|clusters|network|core|node-state|security|telegram-settings|user-templates|route-credentials|subscription-catalog|monitoring-thresholds|generated-config|node-runtime-config|sidecar-runtime-config|xray)\.json$'

# Directories a running panel or agent writes into. `data/` and `.smoke/` are the
# documented ones; `.dev-*` catches the pattern the leak came from.
by_directory='(^|/)(data|\.smoke|\.dev[^/]*|dist|sidecar-generated|route-credentials|xray-validation|xray-updates)/'

offenders="$(printf '%s\n' "$tracked" \
  | grep -E "$by_extension|$by_filename|$by_directory" \
  | grep -v '^panel/crates/panel-core/golden/' \
  || true)"

if [ -n "$offenders" ]; then
  printf 'tracked-file check: these are runtime state, not source, and must not be tracked:\n' >&2
  printf '%s\n' "$offenders" | sed 's/^/  /' >&2
  printf '\nAdd the path to .gitignore and remove it with `git rm --cached`.\n' >&2
  printf 'If a file genuinely belongs in the repository, widen the exclusion in\n' >&2
  printf 'scripts/check-tracked-files.sh deliberately rather than around it.\n' >&2
  exit 1
fi

printf 'No runtime state is tracked (%s files checked).\n' "$(printf '%s\n' "$tracked" | wc -l | tr -d ' ')"
