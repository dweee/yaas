#!/usr/bin/env python3
"""Reject Nix store dependencies in an extracted Linux AppImage."""

from pathlib import Path
import subprocess
import sys


def verify(root: Path) -> int:
    count = 0
    for path in root.rglob("*"):
        if path.is_symlink():
            if "/nix/store/" in str(path.readlink()):
                raise ValueError(f"Nix store symlink: {path}")
            continue
        if not path.is_file():
            continue
        with path.open("rb") as stream:
            if stream.read(4) != b"\x7fELF":
                continue
        result = subprocess.run(
            ["readelf", "--wide", "--program-headers", "--dynamic", str(path)],
            check=True, capture_output=True, text=True,
        )
        if "/nix/store/" in result.stdout:
            raise ValueError(f"Nix store interpreter or dynamic dependency: {path}")
        count += 1
    if not count:
        raise ValueError("No ELF files found in the AppImage")
    launcher = root / "AppRun"
    if "/nix/store/" in launcher.read_text():
        raise ValueError("AppRun depends on the Nix store")
    print(f"Verified {count} ELF files: no Nix store loader or dynamic dependencies")
    return count


if __name__ == "__main__":
    verify(Path(sys.argv[1]))
