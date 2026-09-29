#!/usr/bin/env python3
"""Toolchain and supply-chain hygiene checks for the CI (audit findings CI-1, CI-2).

Rules (one line per violation; exit status 1 when any rule fails):
  R1  every workflow declares a top-level `permissions:` block;
  R2  `contents: write` is never granted at workflow level;
  R3  every `uses:` is pinned to a full 40-hex commit SHA followed by a `# <ref>` comment;
  R4  every actions/checkout step sets `persist-credentials: false`;
  R5  `cargo install` always uses `--locked` and never installs from a git URL;
  R6  `cargo build|check|clippy|doc|test` always uses `--locked`;
  R7  `cargo fuzz` always runs on nightly (`cargo +nightly fuzz`);
  R8  rust-toolchain.toml `channel` equals Cargo.toml `rust-version` (the MSRV);
  R9  a numeric `toolchain:` input in a workflow equals the MSRV.

Standard library only, so it runs locally and on any GitHub runner.
Usage: python3 scripts/check_workflows.py [WORKFLOW.yml ...]
       (default: every .github/workflows/*.yml)
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
USES_RE = re.compile(r"^\s*(?:-\s+)?uses:\s*(?P<ref>[^\s#]+)\s*(?P<comment>#.*)?$")
PINNED_RE = re.compile(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_./-]+@[0-9a-f]{40}$")
CARGO_INSTALL_RE = re.compile(r"\bcargo\s+(?:\+\S+\s+)?install\b")
CARGO_LOCKED_CMD_RE = re.compile(r"\bcargo\s+(?:\+\S+\s+)?(?:build|check|clippy|doc|test)\b")
CARGO_FUZZ_RE = re.compile(r"\bcargo\s+(?:\+(?P<toolchain>\S+)\s+)?fuzz\b")
TOOLCHAIN_INPUT_RE = re.compile(r"^\s*toolchain:\s*\"?(?P<version>\d+\.\d+(?:\.\d+)?)\"?\s*$")


def read_msrv() -> tuple[str | None, list[str]]:
    manifest = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    toolchain = (ROOT / "rust-toolchain.toml").read_text(encoding="utf-8")
    msrv = re.search(r'^rust-version\s*=\s*"([^"]+)"', manifest, re.MULTILINE)
    channel = re.search(r'^channel\s*=\s*"([^"]+)"', toolchain, re.MULTILINE)
    msrv_value = msrv.group(1) if msrv else None
    channel_value = channel.group(1) if channel else None
    if msrv_value is None or channel_value != msrv_value:
        return msrv_value, [
            f"rust-toolchain.toml: R8 channel {channel_value!r} != Cargo.toml rust-version {msrv_value!r}"
        ]
    return msrv_value, []


def check_file(path: Path, msrv: str | None) -> list[str]:
    lines = path.read_text(encoding="utf-8").splitlines()
    name = path.name
    errors: list[str] = []
    if not any(line.startswith("permissions:") for line in lines):
        errors.append(f"{name}: R1 missing top-level 'permissions:' block")
    for index, line in enumerate(lines):
        where = f"{name}:{index + 1}"
        if line.lstrip().startswith("#"):
            continue
        if re.match(r"^  contents:\s*write\b", line):
            errors.append(f"{where}: R2 'contents: write' granted at workflow level")
        uses = USES_RE.match(line)
        if uses:
            ref = uses.group("ref")
            pinned = PINNED_RE.match(ref) and uses.group("comment")
            if not ref.startswith("./") and not pinned:
                errors.append(f"{where}: R3 action not pinned to a commit SHA + '# <ref>': {ref}")
            if ref.startswith("actions/checkout@"):
                window = lines[index + 1 : index + 5]
                if not any("persist-credentials: false" in w for w in window):
                    errors.append(f"{where}: R4 actions/checkout without 'persist-credentials: false'")
        if CARGO_INSTALL_RE.search(line):
            if "--locked" not in line:
                errors.append(f"{where}: R5 'cargo install' without --locked")
            if "--git" in line:
                errors.append(f"{where}: R5 'cargo install' from a git URL")
        elif CARGO_LOCKED_CMD_RE.search(line) and "--locked" not in line:
            errors.append(f"{where}: R6 cargo command without --locked: {line.strip()}")
        fuzz = CARGO_FUZZ_RE.search(line)
        if fuzz and fuzz.group("toolchain") != "nightly":
            errors.append(f"{where}: R7 'cargo fuzz' must run as 'cargo +nightly fuzz'")
        toolchain = TOOLCHAIN_INPUT_RE.match(line)
        if toolchain and toolchain.group("version") != msrv:
            errors.append(f"{where}: R9 toolchain {toolchain.group('version')} != MSRV {msrv}")
    return errors


def main(argv: list[str]) -> int:
    paths = [Path(arg) for arg in argv] or sorted((ROOT / ".github" / "workflows").glob("*.yml"))
    msrv, errors = read_msrv()
    errors += [error for path in paths for error in check_file(path, msrv)]
    for error in errors:
        print(error)
    if errors:
        print(f"FAIL: {len(errors)} violation(s) in {len(paths)} workflow file(s)")
        return 1
    print(f"OK: {len(paths)} workflow file(s) pass")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
