#!/usr/bin/env bash
set -euo pipefail

# Nix supplies the launcher tools; Ubuntu supplies the distributable ABI.
# Usage: nix run .#portable-build -- [output-path]
if [[ $(uname -m) != x86_64 || $(uname -s) != Linux ]]; then
  echo "The portable build currently supports x86_64 Linux hosts." >&2
  exit 1
fi
root=$(git rev-parse --show-toplevel)
cd "$root"
output=$(realpath -m "${1:-dist/yaas-nix.AppImage}")
work="$root/build/portable-nix"
mkdir -p "$work/source" "$work/cache/home" "$work/cache/build" "$work/cache/target" "$(dirname "$output")"

if ! docker info >/dev/null 2>&1; then
  echo "Start Docker and ensure your user can access its daemon." >&2
  exit 1
fi

# Copy tracked and unignored new files, including the current uncommitted work.
# Generated build output and ignored credentials are not build inputs.
python3 - "$root" "$work/source" <<'PY'
from pathlib import Path
import shutil
import subprocess
import sys

root, destination = map(Path, sys.argv[1:])
shutil.rmtree(destination)
destination.mkdir()
files = subprocess.check_output(
    ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"], cwd=root
).split(b"\0")
for raw in files:
    if not raw:
        continue
    relative = Path(raw.decode())
    source = root / relative
    if not source.exists() and not source.is_symlink():
        continue
    target = destination / relative
    target.parent.mkdir(parents=True, exist_ok=True)
    if source.is_symlink():
        if target.exists() or target.is_symlink():
            target.unlink()
        target.symlink_to(source.readlink())
    else:
        shutil.copy2(source, target)
PY

image="yaas-portable-builder:$(id -u)-$(id -g)"
docker build --build-arg "BUILD_UID=$(id -u)" --build-arg "BUILD_GID=$(id -g)" \
  -t "$image" -f "$root/nix/portable.Dockerfile" "$root/nix"
docker run --rm --user "$(id -u):$(id -g)" \
  --mount "type=bind,source=$work/source,target=/workspace" \
  --mount "type=bind,source=$work/cache,target=/cache" \
  --mount "type=bind,source=$work/cache/build,target=/workspace/build" \
  --mount "type=bind,source=$work/cache/target,target=/workspace/target" \
  --env HOME=/cache/home --env CARGO_HOME=/cache/cargo \
  --env YAAS_RELEASE_CHANNEL=development \
  "$image" bash -c 'scripts/build_appimage.sh dist/yaas.AppImage'

install -m755 "$work/source/dist/yaas.AppImage" "$output"
printf 'Portable AppImage: %s\n' "$output"
printf 'Run with portable data: %s --portable\n' "$output"
