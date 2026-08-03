# Testing policy

Two rules, both written after the same failure happened repeatedly: a check
passed while the thing it was supposed to protect did not work.

## A validator is tested through the entry point production uses

A test must reach a validation rule the way a request reaches it — through the
handler, the document builder, or whatever public path the rule actually sits
behind. Calling the validator function directly is not allowed.

The reason is not tidiness. A direct call proves the function returns an error
for bad input. It does not prove the function is reached. If a violation cannot
be constructed through the public path, the rule is not connected to anything,
and a test calling it directly reports success for a rule that never runs.

This has now happened three times in this project:

- `/api/ui/contracts` advertised a route that did not exist. The document was a
  hand-written literal, so nothing compared it against the router.
- Shadowsocks was reported as production-ready while the renderer emitted a
  config Xray rejected. The real-Xray test skipped silently when no binary was
  configured, and the protocol list it checked was hardcoded rather than derived.
- The six XHTTP semantic rules had their own tests and were never called from
  config validation at all. Every rule passed its test; none of them ran.

The corollary is the useful half: **if you cannot construct the violation through
the public path, do not write the direct test — connect the rule.** The attempt
to write the test through the entry point is what reveals that the rule is
orphaned.

`validator_rules_are_not_tested_in_isolation` in `panel-core` enforces this. It
scans the crate for calls to `validate_*` from inside `#[cfg(test)]` modules and
fails on anything not in its allowlist. The allowlist carries the rules that
predate this policy; it may shrink and must never grow.

### What is not a violation

A function that *is* the production entry point may be called directly — that is
the entry point, not a bypass. The distinction is whether a handler calls the
function, or whether the function is what the handler calls.

## Green tests on a warm target directory prove nothing

Both workspaces passed locally and both CI jobs failed on the first real run, for
two unrelated reasons that a populated `target/` and a previously built frontend
had hidden:

- `panel-app` embedded `web/dist/index.html` with `include_str!`. That path is a
  build output and is not in the repository, so the crate did not compile from a
  clean checkout at all. It compiled locally because the bundle was left over
  from an earlier run.
- `node-core` tests execute a stub built from an example. `cargo test` compiles
  examples only to hash-suffixed paths; the plain name the fixtures copy appears
  only from `cargo build --examples`. It existed locally from an earlier plain
  `cargo test`.

Neither is visible to any test, because neither is a property of the code. Both
are properties of the working directory.

What follows:

- CI builds from a clean checkout, and that is the only run whose result counts.
  A local green suite is a fast signal, not evidence.
- Anything that depends on a build artifact — a bundled frontend, a fixture
  binary, a packaged release — needs a check that constructs the artifact and
  exercises it, rather than a test that assumes it is present.
- A new check must be shown to fail. `verify-release-package.sh` passed on its
  first run against a package containing no frontend, because the release binary
  still found the bundle in the build host's source tree. The check was worthless
  until the fallback was restricted to debug builds and the failure reproduced.

## Checks that cannot fail

The last point generalises. Before trusting a new check, make it fail on purpose:
break the thing it guards and confirm it goes red. A check that has never been
observed failing is an assertion about the author's intent, not about the system.

The same reasoning covers skips. `HYDRA_REQUIRE_XRAY_TEST=1` in CI forbids the
real-Xray test from skipping when no binary is configured, because a silent skip
is indistinguishable from a pass in the summary line — and that silent skip is
what let a removed `allowInsecure` flag and a missing Reality renderer survive
green runs.
