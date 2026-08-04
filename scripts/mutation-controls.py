#!/usr/bin/env python3
"""Runs the negative controls for this repository's guards.

Every guard here was written after something got past it, and a guard that has
never been seen failing is a claim about the author's intent. These controls
break each guarded property on purpose and require the guard to notice.

The harness exists because doing this by hand went wrong three times in a row.
Each control was assembled ad hoc, and three of them reported green while
changing nothing: a pattern that no longer matched after rustfmt reflowed the
code, a marker deleted from a list that the failing case did not depend on, a
threshold edited in a file the test did not read. A control that silently
mutates nothing is worse than no control, because it is filed as evidence.

So the mutation is applied through one function that refuses to proceed unless
it matched exactly once, and the file is restored whether the command passes,
fails or raises.

    python3 scripts/mutation-controls.py           # run all
    python3 scripts/mutation-controls.py memory    # run those whose name matches
"""

from __future__ import annotations

import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent


@dataclass
class Control:
    """One guard, the break that must trip it, and how to ask."""

    name: str
    why: str
    path: str
    find: str
    replace: str
    command: list[str]
    cwd: str = "."
    # Text the failure output must contain. Without this a control passes when
    # the command fails for an unrelated reason — a compile error, a missing
    # binary — which looks identical to the guard working.
    expect_in_output: str = ""
    extra_files: dict[str, str] = field(default_factory=dict)


CONTROLS: list[Control] = [
    Control(
        name="memory-units",
        why="sysinfo returns bytes; the KiB multiply from 0.29 made every "
        "machine-wide figure 1024 times too large",
        path="panel/crates/panel-core/src/lib.rs",
        find="    let memory_total_bytes = system.total_memory();",
        replace="    let memory_total_bytes = system.total_memory().saturating_mul(1024);",
        command=["cargo", "test", "-p", "panel-core", "reported_memory_matches_proc_meminfo"],
        cwd="panel",
        expect_in_output="total memory is not in bytes",
    ),
    Control(
        name="golden-used-vs-total",
        why="a normalized number is checked by nothing, but the relation between "
        "two of them survives normalization",
        path="panel/crates/panel-core/src/lib.rs",
        find="    let memory_used_bytes = system.used_memory();",
        replace="    let memory_used_bytes = system.used_memory().saturating_mul(1024);",
        command=[
            "cargo",
            "test",
            "-p",
            "panel-core",
            "ui_surface_documents_match_golden_snapshots",
        ],
        cwd="panel",
        expect_in_output="a measurement cannot exceed its own total",
    ),
    Control(
        name="golden-route-paths",
        why="ui_contracts exists to publish route paths; the substring rule "
        "blanked all 168 of them",
        path="panel/crates/panel-core/src/lib.rs",
        find='            || key.ends_with("_path")',
        replace='            || key.contains("path")',
        command=[
            "cargo",
            "test",
            "-p",
            "panel-core",
            "stateless_documents_match_golden_snapshots",
        ],
        cwd="panel",
        expect_in_output="the document shape changed",
    ),
    Control(
        name="secret-shape-lowercase-hex",
        why="the leaked subscription tokens were lowercase hex, which the "
        "mixed-case rule could not see",
        path="panel/crates/panel-core/src/lib.rs",
        find="        if trimmed.len() >= 32\n            && trimmed.chars().all(|symbol| symbol.is_ascii_hexdigit())",
        replace="        if trimmed.len() >= 96\n            && trimmed.chars().all(|symbol| symbol.is_ascii_hexdigit())",
        command=["cargo", "test", "-p", "panel-core", "secret_guard_rejects_credential_material"],
        cwd="panel",
        expect_in_output="the shape rule no longer covers what leaked",
    ),
    Control(
        name="secret-canaries-are-shared",
        why="the golden guard and the tracked-content check must read one list, "
        "or the two drift",
        path="scripts/secret-canaries.txt",
        find="key token\n",
        replace="",
        command=["cargo", "test", "-p", "panel-core", "secret_guard_rejects_credential_material"],
        cwd="panel",
        expect_in_output="rejected on the key alone",
    ),
    Control(
        name="domain-dependencies",
        why="a crate that compiles for wasm32 and traps in the browser is "
        "invisible to the build",
        path="panel/crates/panel-domain/Cargo.toml",
        find="thiserror.workspace = true",
        replace="thiserror.workspace = true\nsysinfo.workspace = true",
        command=["cargo", "test", "-p", "panel-domain", "dependencies_stay_minimal"],
        cwd="panel",
        expect_in_output="sysinfo",
    ),
    Control(
        name="domain-host-only-apis",
        why="std::fs compiles for wasm32 into a stub and fails at run time",
        path="panel/crates/panel-domain/src/system.rs",
        find="#[derive(Debug, Clone, Serialize, Deserialize)]\npub struct SystemOverview {",
        replace="pub fn read_budget() -> String {\n    std::fs::read_to_string(\"/etc/hydra\").unwrap_or_default()\n}\n\n#[derive(Debug, Clone, Serialize, Deserialize)]\npub struct SystemOverview {",
        command=["cargo", "test", "-p", "panel-domain", "domain_source_is_free_of_host_only_apis"],
        cwd="panel",
        expect_in_output="system.rs: std::fs",
    ),
    Control(
        name="validator-entry-points",
        why="an entry point named in UNCONVERTED that no longer exists leaves "
        "the table pointing at nothing",
        path="panel/crates/panel-core/src/lib.rs",
        find='("validate_username", "create_user"),',
        replace='("validate_username", "create_user_renamed_away"),',
        command=["cargo", "test", "-p", "panel-core", "validator_rules_are_not_tested_in_isolation"],
        cwd="panel",
        expect_in_output="no longer exist",
    ),
    Control(
        name="xhttp-render-accepted",
        why="xhttp was added to the transport enum and the renderer without "
        "being added to the validator, so the panel rejected its own output",
        path="panel/crates/panel-core/src/lib.rs",
        find='"tcp" | "udp" | "ws" | "grpc" | "httpupgrade" | "quic" | "xhttp"',
        replace='"tcp" | "udp" | "ws" | "grpc" | "httpupgrade" | "quic"',
        command=["cargo", "test", "-p", "panel-core", "raw_validation_accepts_the_rendered_xhttp_stream"],
        cwd="panel",
        expect_in_output="rejected its own XHTTP render",
    ),
    Control(
        name="canary-file-holds-no-values",
        why="the canary list is exempt from the content scan, so it is where a "
        "secret would be safe from it",
        path="scripts/secret-canaries.txt",
        find="key token",
        replace="key token\nliteral c441b457900d57c8562586abfe25f2693c7237a61bd046c8",
        command=["python3", "scripts/check-tracked-content.py"],
        expect_in_output="over the 24 a marker may need",
    ),
    Control(
        name="tracked-content-finds-a-planted-token",
        why="the path guard matches on what a file is; a token in fixture.json "
        "passes it",
        path="scripts/secret-canaries.txt",
        find="key token",
        replace="key token",
        command=["python3", "scripts/check-tracked-content.py"],
        expect_in_output="lowercase hex characters",
        extra_files={
            # Named innocuously and placed where nothing suspects it, under a key
            # that is not in the canary list — so only the shape rule can catch it.
            "panel/docs/examples/fixture.json": '{\n  "sub_key": "f27cc679bfc5f178919815a9ff14b6b27fd45a3605bd27fb"\n}\n'
        },
    ),
]


def apply_once(path: Path, find: str, replace: str) -> str:
    """Replaces `find` with `replace`, refusing unless it matched exactly once.

    Returns the original text so the caller can restore it. The exactly-once
    requirement is the whole point: a mutation that matched zero times leaves the
    guard measuring an unchanged repository and reporting success.
    """
    original = path.read_text()
    occurrences = original.count(find)
    if occurrences != 1:
        raise AssertionError(
            f"{path}: the mutation matched {occurrences} times, expected exactly 1. "
            f"The control would have tested nothing."
        )
    path.write_text(original.replace(find, replace, 1))
    return original


def run(control: Control) -> tuple[bool, str]:
    path = REPO_ROOT / control.path
    written: list[Path] = []
    original: str | None = None
    try:
        original = apply_once(path, control.find, control.replace)
        for name, body in control.extra_files.items():
            extra = REPO_ROOT / name
            extra.parent.mkdir(parents=True, exist_ok=True)
            extra.write_text(body)
            written.append(extra)
            subprocess.run(["git", "add", "-f", name], cwd=REPO_ROOT, check=True, capture_output=True)

        result = subprocess.run(
            control.command,
            cwd=REPO_ROOT / control.cwd,
            capture_output=True,
            text=True,
        )
        output = result.stdout + result.stderr

        if result.returncode == 0:
            return False, "the guard did not notice the break"
        if control.expect_in_output and control.expect_in_output not in output:
            return False, (
                f"the command failed, but not for the guarded reason: "
                f"{control.expect_in_output!r} is absent from the output"
            )
        return True, ""
    finally:
        if original is not None:
            path.write_text(original)
        for extra in written:
            subprocess.run(
                ["git", "rm", "-q", "--cached", str(extra.relative_to(REPO_ROOT))],
                cwd=REPO_ROOT,
                capture_output=True,
            )
            extra.unlink(missing_ok=True)
            if not any(extra.parent.iterdir()):
                extra.parent.rmdir()


def main() -> int:
    selector = sys.argv[1] if len(sys.argv) > 1 else ""
    selected = [c for c in CONTROLS if selector in c.name]
    if not selected:
        print(f"no control matches {selector!r}", file=sys.stderr)
        return 1

    failures = 0
    for control in selected:
        passed, detail = run(control)
        if passed:
            print(f"  ok      {control.name}")
        else:
            failures += 1
            print(f"  FAILED  {control.name}: {detail}")
            print(f"          guards: {control.why}")

    print(
        f"\n{len(selected) - failures}/{len(selected)} controls tripped their guard."
    )
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
