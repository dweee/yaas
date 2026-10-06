# Portable Linux builds with Nix

From the YAAS checkout on an x86_64 Linux host, with Nix flakes enabled and a
running Docker daemon accessible to your user:

```sh
nix run .#portable-build
# Or choose an output path:
nix run .#portable-build -- dist/YAAS-linux-x86_64.AppImage
# Equivalent just recipe:
just build-portable
```

Run the result with:

```sh
chmod +x dist/yaas-nix.AppImage
./dist/yaas-nix.AppImage --portable
```

YAAS stores settings, downloaded tools, and application data in `_portable_data`
alongside the AppImage. Nix and Docker are build requirements, not application
runtime requirements. Linux hosts still need a working graphics stack and an
AppImage-compatible environment; on NixOS, use `appimage-run` or the host's
AppImage support.

## Build environment

`flake.lock` pins the Nix tools that launch the build. `nix/portable.Dockerfile`
pins the Ubuntu image digest, Flutter 3.47.0 commit, Rust 1.98.1, Rinf CLI 8.10.1,
Fastforge 0.6.12, and the appimagetool checksum. The Ubuntu container uses the
existing `scripts/build_appimage.sh` pipeline to build Flutter and Rust, package
native libraries, and bundle ADB, 7-Zip, and the updater. Building against the
Ubuntu ABI avoids distributing ELF loaders and libraries from `/nix/store`.
The pipeline checks ELF interpreters, dynamic dependencies, and symlinks for
Nix store references before publishing the artifact.

This is a Nix-controlled container build, rather than a sandboxed `nix build`
derivation for the application. It needs network access: Ubuntu packages,
Flutter engine artifacts, ADB, and some dependency downloads are still fetched
from their upstream repositories. Pinning the main tools does not make this a
fully offline or bit-for-bit reproducible build. The glibc baseline is Ubuntu
24.04; older distributions are not guaranteed to work.

The launcher snapshots tracked and unignored new files, including uncommitted
changes, into `build/portable-nix/source`. It does not run generators in your
checkout. Build and Rust caches live in `build/portable-nix/cache`; the Docker
builder image is cached under `yaas-portable-builder:<uid>-<gid>`. Repeated builds
reuse those caches. Build artifacts are ignored by Git.

The provider API key is not passed to the builder or embedded in the AppImage.
Supply it in `YAAS_PUBLIC_SERVER_API_KEY` when launching the app, as described
in [public_server.md](public_server.md).

Validate the Nix launcher and shell script with `nix flake check`.
