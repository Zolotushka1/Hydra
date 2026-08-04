#!/usr/bin/env python3
"""Refuses to let credential material become a tracked file's contents.

The sibling check, check-tracked-files.sh, matches on what a file *is*: its
extension, its name, the shape of the directory holding it. That is blind to the
case it cannot see — a token pasted into fixture.json, example.yaml or a README.
The leak it was written after was a value, not a filename.

Canaries come from scripts/secret-canaries.txt, the same list the golden secret
guard in panel-core reads. Keeping one list is the point: `subscription_token`
was already a canary for golden documents when three of them were committed
inside users.json.

What counts as a finding:

  * a key whose name carries a canary marker, assigned a literal that looks like
    credential material — long, and drawn from the alphabet secrets use;
  * a literal canary anywhere.

Mentioning the word "token" in prose is not a finding, and neither is a type
declaration. The guard is about assigned values.
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
CANARY_FILE = REPO_ROOT / "scripts" / "secret-canaries.txt"
ALLOWLIST_FILE = REPO_ROOT / "scripts" / "secret-canary-allowlist.txt"

# The alphabet credential material is drawn from: base64, base64url, hex.
CREDENTIAL_ALPHABET = r"A-Za-z0-9+/=_-"

# Sixteen is below any real token and above every identifier that appears as an
# assigned literal in this repository. The leaked subscription tokens were 48.
MINIMUM_CREDENTIAL_LENGTH = 16


IDENTIFIER = re.compile(r"^[a-z0-9]+(?:_[a-z0-9]+)+$")


def looks_random(value: str) -> bool:
    """Distinguishes a secret from a name that happens to be long.

    Credential material is drawn without structure, so it mixes digits with
    letters and uses most of its alphabet. A name does neither: `two_factor_
    secret_base32` is long, but it is words joined by underscores.

    The leaked subscription tokens were 48 lowercase hex characters — no
    underscores, fifteen distinct symbols — which is why "not an identifier" is
    checked by shape rather than by case.
    """
    if IDENTIFIER.match(value):
        return False
    if not any(symbol.isdigit() for symbol in value):
        return False
    if not any(symbol.isalpha() for symbol in value):
        return False
    return len(set(value)) >= 10


def load_canaries() -> tuple[list[str], list[str]]:
    keys: list[str] = []
    literals: list[str] = []
    for line in CANARY_FILE.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        kind, _, value = line.partition(" ")
        if kind == "key":
            keys.append(value)
        elif kind == "literal":
            literals.append(value)
        else:
            raise SystemExit(f"unknown canary kind {kind!r} in {CANARY_FILE}")
    if not keys or not literals:
        raise SystemExit(f"{CANARY_FILE} produced no canaries, so this check is not working")
    return keys, literals


def load_allowlist() -> set[str]:
    """Values this repository contains on purpose.

    Synthetic material in tests, and nothing else. Each entry is the exact
    string, so allowing one fixture cannot accidentally allow a real secret that
    resembles it.
    """
    if not ALLOWLIST_FILE.exists():
        return set()
    return {
        line.strip()
        for line in ALLOWLIST_FILE.read_text().splitlines()
        if line.strip() and not line.strip().startswith("#")
    }


def tracked_files() -> list[Path]:
    listing = subprocess.run(
        ["git", "ls-files", "-z"],
        cwd=REPO_ROOT,
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    return [REPO_ROOT / name for name in listing.split("\0") if name]


def main() -> int:
    keys, literals = load_canaries()
    allowed = load_allowlist()

    # The value must be a quoted literal. An unquoted match is an expression —
    # `subscription_token: user.subscription_token.clone()` — and a guard that
    # flags those reports most of the codebase, gets switched off, and protects
    # nothing.
    key_pattern = re.compile(
        r"""(?P<key>[A-Za-z0-9_.-]*(?:%s)[A-Za-z0-9_.-]*)   # a key carrying a marker
            ["']?\s*[:=]\s*                                  # assigned, key may be quoted
            ["'](?P<value>[%s]{%d,})["']                     # to a quoted literal
        """
        % ("|".join(re.escape(marker) for marker in keys), CREDENTIAL_ALPHABET, MINIMUM_CREDENTIAL_LENGTH),
        re.VERBOSE | re.IGNORECASE,
    )

    findings: list[str] = []
    scanned = 0

    for path in tracked_files():
        try:
            text = path.read_text(encoding="utf-8")
        except (UnicodeDecodeError, FileNotFoundError, IsADirectoryError):
            continue
        scanned += 1

        for number, line in enumerate(text.splitlines(), start=1):
            for literal in literals:
                if literal in line and line.strip() not in allowed:
                    relative = path.relative_to(REPO_ROOT)
                    findings.append(f"{relative}:{number}: literal canary {literal!r}")

            for match in key_pattern.finditer(line):
                value = match.group("value")
                if value in allowed or not looks_random(value):
                    continue
                relative = path.relative_to(REPO_ROOT)
                findings.append(
                    f"{relative}:{number}: {match.group('key')} assigned "
                    f"{len(value)} credential-shaped characters"
                )

    if findings:
        print("tracked-content check: credential material in tracked files:", file=sys.stderr)
        for finding in findings:
            print(f"  {finding}", file=sys.stderr)
        print(
            "\nIf a value is synthetic and belongs here, add the exact string to\n"
            "scripts/secret-canary-allowlist.txt with a reason. Do not widen the\n"
            "canary list to make this pass.",
            file=sys.stderr,
        )
        return 1

    print(f"No credential material in tracked contents ({scanned} text files scanned).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
