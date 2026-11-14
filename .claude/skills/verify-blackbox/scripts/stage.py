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


def newest(binaries):
    """The most recently built of `binaries` that exist, or None."""
    found = [b for b in binaries if b.is_file()]
    return max(found, key=lambda b: b.stat().st_mtime, default=None)


def ours(target, root, chosen):
    """Whether `target` holds a build of the tree at `root`: its own target/, the one
    CARGO_TARGET_DIR names (`chosen`), or a shared one whose `.bounded-tree` (written by the
    maintainer's `bounded` runner) names `root` as the tree that built there last."""
    if target in {root / "target", chosen}:
        return True
    try:
        return (target / ".bounded-tree").read_text().strip() == str(root)
    except OSError:
        return False


def stage(name):
    root = Path(git(Path.cwd(), "rev-parse", "--show-toplevel"))
    # A worktree's build may land in the main checkout's target/ (`bounded` puts it there).
    common = Path(git(root, "rev-parse", "--path-format=absolute", "--git-common-dir"))
    chosen = Path(os.environ["CARGO_TARGET_DIR"]) if os.environ.get("CARGO_TARGET_DIR") else None
    targets = [t for t in (chosen, root / "target", common.parent / "target") if t]
    binary = newest([t / "debug/meada" for t in targets if ours(t, root, chosen)])
    if binary is None:
        looked = ", ".join(str(t) for t in targets)
        sys.exit(f"no debug/meada built from this tree in {looked}: run `cargo build` first")
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
        other = tmp / "other-meada"
        other.write_text("")
        os.utime(other, (4000, 4000))
        assert newest([binary, other, tmp / "missing"]) == other
        assert newest([tmp / "missing"]) is None
        own, shared = tmp / "tree", tmp / "main/target"
        shared.mkdir(parents=True)
        assert ours(own / "target", own, None) and ours(shared, own, shared)
        assert not ours(shared, own, None)
        (shared / ".bounded-tree").write_text(f"{tmp / 'other'}\n")
        assert not ours(shared, own, None)
        (shared / ".bounded-tree").write_text(f"{own}\n")
        assert ours(shared, own, None)
    print("stage.py selftest passed")


if __name__ == "__main__":
    if sys.argv[1:] == ["--selftest"]:
        selftest()
    elif len(sys.argv) == 2 and "/" not in sys.argv[1] and sys.argv[1] not in {"", ".", ".."}:
        stage(sys.argv[1])
    else:
        sys.exit(__doc__)
