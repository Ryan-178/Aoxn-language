#!/usr/bin/env python3
"""Read-only, dependency-free Markdown path audit; never fetch or run doc commands."""
from __future__ import annotations

import argparse
from collections import Counter
from dataclasses import dataclass
import os
from pathlib import Path
import re
import subprocess
from urllib.parse import unquote, urlsplit

# Explicit historical-document allowlist, not a blanket docs/ exclusion.
HISTORICAL_REPORTS = frozenset({
    "CHANGELOG.md",  # release history, not current command instructions
    "docs/selfhost.md",  # v0.19 assessment with a current-status preface
    "docs/optimization-report.md",  # LLVM-era measurements
    "docs/llvm-independence-report.md",  # backend migration record
})
SKIP_DIRS = frozenset({".git", "target", "node_modules", "wiki", "__pycache__",
                       ".zcode", "LLVM", "third-party-package-in-the-making"})
CATEGORIES = ("missing-path", "historical-path", "command-drift")
LINK = re.compile(r"!?\[[^\]\n]*\]\(\s*(<[^>\n]*>|(?:[^\s()]|\([^()]*\))*)(?:\s+\"[^\"\n]*\"|\s+'[^'\n]*')?\s*\)")
DEFINITION = re.compile(r"^\s{0,3}\[([^\]]+)\]:\s*(<[^>]+>|\S+)", re.M)
REFERENCE = re.compile(r"!?\[([^\]\n]+)\](?:\[([^\]\n]*)\])?")
COMMAND = re.compile(r"(?:^|[\s`$>])(?:cargo\s+(?:run|test|build)|(?:aoxn|Aoxn)\s+(?:run|build|check|c|symbols)|python(?:3)?\s|bash\s|pwsh\s|powershell\s)")
COMMAND_PATH = re.compile(r"(?<![\w.])(?:\.{1,2}[/\\])?(?:[\w.-]+[/\\])*[\w.-]+\.(?:ax|tsx?|rs|py|ps1|sh|mjs|cjs)(?![\w.])")


@dataclass(frozen=True)
class Finding:
    category: str
    source: str
    line: int
    target: str
    detail: str


def markdown_files(root: Path):
    """Prune before descending; never follow symlink directories/files."""
    for base, dirs, files in os.walk(root, followlinks=False):
        dirs[:] = sorted(d for d in dirs if d not in SKIP_DIRS
                         and not (Path(base) / d).is_symlink())
        for name in sorted(files):
            path = Path(base) / name
            if path.suffix.lower() == ".md" and not path.is_symlink():
                yield path


def historical_paths(root: Path) -> set[str]:
    """Deleted tracked paths from local history only (no fetch, no shell)."""
    try:
        result = subprocess.run(
            ["git", "-C", str(root), "-c", "core.quotepath=false", "log", "--all",
             "--diff-filter=D", "--format=", "--name-only", "-z"],
            capture_output=True, check=False,
        )
    except OSError as exc:
        raise RuntimeError(f"cannot read local Git history: {exc}") from exc
    if result.returncode:
        raise RuntimeError("cannot read local Git history (use a Git repository as --root)")
    return {p.strip() for p in result.stdout.decode("utf-8", errors="replace").split("\0") if p.strip()}


def visible_lines(text: str):
    # Keep line numbers through multi-line comments; skip fenced/indented code
    # for Markdown links, but inspect code for command hints separately.
    text = re.sub(r"<!--.*?-->", lambda m: "\n" * m[0].count("\n"), text, flags=re.S)
    fence = None
    for number, line in enumerate(text.splitlines(), 1):
        marker = re.match(r"^\s{0,3}(`{3,}|~{3,})", line)
        if marker:
            token = marker[1]
            if fence is None:
                fence = token
            elif token[0] == fence[0] and len(token) >= len(fence):
                fence = None
            yield number, line, ""
        elif fence or line.startswith(("    ", "\t")):
            yield number, line, ""
        else:
            yield number, line, re.sub(r"(`+).*?\1", "", line)


def local_target(root: Path, source: Path, target: str):
    target = target.strip("<>")
    # Windows drive paths and root-relative web URLs are not relative links.
    if not target or target.startswith(("#", "/", "\\")) or re.match(r"^[A-Za-z]:", target):
        return None
    url = urlsplit(target)
    if url.scheme or url.netloc:
        return None
    value = unquote(url.path).replace("\\", "/")
    if not value:
        return None
    path = (source.parent / value).resolve()
    try:
        name = path.relative_to(root).as_posix()
    except ValueError:
        return None  # outside repository: not evidence of repository drift
    return path, name


def audit(root: Path, history: set[str]):
    findings = []
    scanned = excluded = 0
    for source in markdown_files(root):
        relative = source.relative_to(root).as_posix()
        if relative in HISTORICAL_REPORTS:
            excluded += 1
            continue
        scanned += 1
        lines = list(visible_lines(source.read_text(encoding="utf-8-sig")))
        definitions = {}
        for _, _, prose in lines:
            match = DEFINITION.match(prose)
            if match:
                definitions[" ".join(match[1].lower().split())] = match[2]
        seen = set()

        def check(number, target, command=False):
            resolved = local_target(root, source, target)
            if resolved is None:
                return
            path, name = resolved
            # Commands can be documented after `cd` into the document directory.
            # Accept either location; do not infer shell state or execute commands.
            if command and path.exists():
                return
            if command:
                resolved = local_target(root, root / "_.md", target)
                if resolved is None:
                    return
                path, name = resolved
            if path.exists():
                return
            category = "historical-path" if name in history else "command-drift" if command else "missing-path"
            key = (number, category, target)
            if key not in seen:
                seen.add(key)
                detail = "deleted path found in local Git history" if category == "historical-path" else "command file token missing; heuristic, review manually" if command else "relative Markdown target does not exist"
                findings.append(Finding(category, relative, number, target, detail))

        for number, raw, prose in lines:
            definition = DEFINITION.match(prose)
            if definition:
                check(number, definition[2])
            else:
                for match in LINK.finditer(prose):
                    check(number, match[1])
                # Reference-style, collapsed and shortcut links; undefined labels
                # are deliberately not treated as filesystem paths.
                without_inline = LINK.sub("", prose)
                for match in REFERENCE.finditer(without_inline):
                    label = match[2] or match[1]
                    target = definitions.get(" ".join(label.lower().split()))
                    if target:
                        check(number, target)
            if COMMAND.search(raw):
                for match in COMMAND_PATH.finditer(raw):
                    check(number, match[0], command=True)
    return findings, scanned, excluded


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--strict", action="store_true", help="exit 1 for any finding, including historical paths and command hints")
    options = parser.parse_args(argv)
    root = options.root.resolve()
    try:
        if not root.is_dir():
            raise RuntimeError("--root must be an existing repository directory")
        findings, scanned, excluded = audit(root, historical_paths(root))
    except (OSError, UnicodeError, RuntimeError, ValueError) as exc:
        print(f"audit-error: {exc}")
        return 2
    for finding in findings:
        print(f"{finding.category}: {finding.source}:{finding.line}: {finding.target} ({finding.detail})")
    counts = Counter(f.category for f in findings)
    print(f"Scanned {scanned} Markdown files; allowlisted {excluded} historical documents; wiki/build/vendor directories pruned.")
    print("; ".join(f"{category}={counts[category]}" for category in CATEGORIES))
    print("Policy: warning-only exits 0; --strict exits 1 for any finding; audit/read errors exit 2. No network or document commands executed.")
    return int(options.strict and bool(findings))


if __name__ == "__main__":
    raise SystemExit(main())
