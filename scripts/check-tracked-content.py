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

# Allowlist value that switches off the lowercase-hex rule for one file. Used for
# generated checksum manifests, where every dependency contributes a sha256 and
# the rule would report hundreds of lines that are not secrets. The other rules
# stay on for those files.
HEX_RULE_OFF = "<generated-checksums>"


IDENTIFIER = re.compile(r"^[a-z0-9]+(?:_[a-z0-9]+)+$")

# A quoted run of lowercase hex, long enough to be key material. Independent of
# any key name, because key names are an open list: `subscription_token` was
# known and `sub_key` in someone else's format is not. The leaked tokens were 48
# characters of exactly this shape.
LOWERCASE_HEX_LITERAL = re.compile(r"""["']([a-f0-9]{32,})["']""")


def is_periodic(value: str) -> bool:
    """True when the string is a shorter run repeated.

    `1a2b3c4d...` written twice is a fixture someone typed. Randomly drawn
    material is not periodic, so this excludes placeholders without excusing
    anything a generator produced.
    """
    return value in (value + value)[1:-1]


def looks_drawn_at_random(value: str) -> bool:
    """Separates key material from a placeholder of the same shape and length.

    Sixty-four `a` characters is the sha256 field of a documentation example.
    Real material uses most of its alphabet and has no period. Neither rule
    excuses anything a CSPRNG produced: the leaked subscription tokens had
    fifteen distinct symbols and no period.
    """
    return len(set(value)) >= 10 and not is_periodic(value)


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


def load_allowlist() -> list[tuple[str, str]]:
    """Values this repository contains on purpose, each scoped to one file.

    Entries are `path<TAB>value`. Scoping matters: the x25519 test vector is
    legitimate in the test that verifies key derivation and is a leak in
    config.json. A global allowance cannot tell those apart.

    This is where the guard switches off, so it is the part that has to stay
    honest. Six entries with reasons is fine; nothing stops a seventh, and an
    entry whose value has vanished from the file it named goes on permitting
    something nobody remembers.
    """
    if not ALLOWLIST_FILE.exists():
        return []
    entries = []
    for number, line in enumerate(ALLOWLIST_FILE.read_text().splitlines(), start=1):
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        path, tab, value = line.partition("\t")
        if not tab:
            raise SystemExit(
                f"{ALLOWLIST_FILE}:{number}: expected `path<TAB>value`, got {line!r}"
            )
        entries.append((path.strip(), value))
    return entries


def is_allowed(entries: list[tuple[str, str]], path: Path, value: str) -> bool:
    relative = str(path.relative_to(REPO_ROOT))
    return any(entry_path == relative and entry_value == value for entry_path, entry_value in entries)


def assert_allowlist_is_alive(entries: list[tuple[str, str]]) -> list[str]:
    """Every entry must still match something in the file it names.

    An entry that matches nothing is not harmless. It is a standing permission
    for a value nobody can point at, and in six months it reads as though
    somebody decided that value was fine here.
    """
    dead = []
    for entry_path, entry_value in entries:
        target = REPO_ROOT / entry_path
        try:
            text = target.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError):
            dead.append(f"{entry_path}: file is gone or unreadable")
            continue
        if entry_value == HEX_RULE_OFF:
            continue
        if entry_value not in text:
            dead.append(f"{entry_path}: no longer contains {entry_value[:48]!r}")
    return dead


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

    dead = assert_allowlist_is_alive(allowed)
    if dead:
        print("tracked-content check: these allowlist entries match nothing:", file=sys.stderr)
        for entry in dead:
            print(f"  {entry}", file=sys.stderr)
        print("\nRemove them. An allowance nobody can point at is not an allowance.", file=sys.stderr)
        return 1

    findings: list[str] = []
    scanned = 0

    for path in tracked_files():
        # The canary list and this file both name the patterns they hunt for.
        if path in (CANARY_FILE, ALLOWLIST_FILE, Path(__file__).resolve()):
            continue
        try:
            text = path.read_text(encoding="utf-8")
        except (UnicodeDecodeError, FileNotFoundError, IsADirectoryError):
            continue
        scanned += 1
        # Whole-file exemption for the hex-shape rule only. Key markers and
        # literal canaries still apply here, so a token assigned to a named field
        # inside one of these files is still a finding.
        hex_rule_exempt = is_allowed(allowed, path, HEX_RULE_OFF)

        for number, line in enumerate(text.splitlines(), start=1):
            for literal in literals:
                if literal in line and not is_allowed(allowed, path, line.strip()):
                    relative = path.relative_to(REPO_ROOT)
                    findings.append(f"{relative}:{number}: literal canary {literal!r}")

            if not hex_rule_exempt:
                for match in LOWERCASE_HEX_LITERAL.finditer(line):
                    value = match.group(1)
                    if not looks_drawn_at_random(value):
                        continue
                    if is_allowed(allowed, path, value) or is_allowed(
                        allowed, path, line.strip()
                    ):
                        continue
                    relative = path.relative_to(REPO_ROOT)
                    findings.append(
                        f"{relative}:{number}: {len(value)} lowercase hex characters, "
                        f"the shape of a token or a key"
                    )

            for match in key_pattern.finditer(line):
                value = match.group("value")
                if is_allowed(allowed, path, value) or not looks_random(value):
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
