#!/usr/bin/env python3
"""Stage the built Meada binary for a black-box verifier: /tmp/verify/<name>/{meada,COMMIT,work/}.

Usage: stage.py <name>  |  stage.py --selftest

Refuses a binary older than a source file, and a tree whose product sources differ from HEAD,
since COMMIT must name what was built. The copy is stripped of debug info (about 470 MB to 60 MB).
"""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

SOURCES = ["src", "meada-core/src", "Cargo.toml", "Cargo.lock", "build.rs"]


def stale(binary, sources):
    """The first source file newer than `binary`, or None."""
    built = binary.stat().st_mtime
    for source in sources:
        files = [source] if source.is_file() else source.rglob("*") if source.is_dir() else []
        for f in files:
            if f.is_file() and f.stat().st_mtime > built:
                return f
    return None


def git(root, *args):
    return subprocess.run(["git", "-C", str(root), *args], check=True, stdout=subprocess.PIPE,
                          text=True).stdout.strip()


def stage(name):
    root = Path(git(Path.cwd(), "rev-parse", "--show-toplevel"))
    binary = root / "target/debug/meada"
    if not binary.is_file():
        sys.exit("no target/debug/meada: run `cargo build` first")
    if git(root, "status", "--porcelain", "--", *SOURCES):
        sys.exit("product sources differ from HEAD: commit them, so COMMIT names what was built")
    newer = stale(binary, [root / s for s in SOURCES])
    if newer:
        sys.exit(f"{newer.relative_to(root)} is newer than the binary: run `cargo build` first")
    dest = Path("/tmp/verify") / name
    (dest / "work").mkdir(parents=True, exist_ok=True)
    if shutil.which("strip"):
        subprocess.run(["strip", "-o", str(dest / "meada"), str(binary)], check=True)
    else:
        shutil.copy2(binary, dest / "meada")
    (dest / "COMMIT").write_text(git(root, "rev-parse", "HEAD") + "\n")
    size = (dest / "meada").stat().st_size >> 20
    print(f"{dest}/meada ({size} MB) at {(dest / 'COMMIT').read_text().strip()}; work dir {dest}/work")


def selftest():
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        (tmp / "src").mkdir()
        source, binary = tmp / "src/lib.rs", tmp / "meada"
        source.write_text("")
        binary.write_text("")
        os.utime(source, (1000, 1000))
        os.utime(binary, (2000, 2000))
        assert stale(binary, [tmp / "src", tmp / "missing.toml"]) is None
        os.utime(source, (3000, 3000))
        assert stale(binary, [tmp / "src"]) == source
    print("stage.py selftest passed")


if __name__ == "__main__":
    if sys.argv[1:] == ["--selftest"]:
        selftest()
    elif len(sys.argv) == 2 and "/" not in sys.argv[1] and sys.argv[1] not in {"", ".", ".."}:
        stage(sys.argv[1])
    else:
        sys.exit(__doc__)
