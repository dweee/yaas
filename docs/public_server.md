# Public server downloader

YAAS supports the public HTTP repository protocol used by VRCD. Add a YAAS
source JSON through the existing downloader source setup. Wrap the provider's
`ServerInfo.json` fields in `public_server`:

```json
{
  "id": "provider-public",
  "display_name": "Provider public server",
  "layout": "public-server",
  "config_update_url": "https://your-config-host.example/yaas.json",
  "rclone_path": "rclone",
  "public_server": {
    "baseUri": "https://your-provider.example/library/",
    "password": "BASE64_ARCHIVE_PASSWORD",
    "api_key_env": "YAAS_PUBLIC_SERVER_API_KEY"
  }
}
```

Use the actual provider URL and base64 password. `config_update_url` must point
to this YAAS configuration, matching the URL entered in source setup. Raw VRCD
JSON is not a YAAS source: it lacks the source identity, update URL, and layout.
`rclone_path` accepts a local binary path/name, a binary or ZIP download URL,
or the existing per-platform map (including `linux-x64`, `windows-x64`, and
`macos-arm64`). A downloaded binary uses YAAS's existing cache. Public sources
use an empty rclone config (`/dev/null` on Unix, `NUL` on Windows).

Set `YAAS_PUBLIC_SERVER_API_KEY` in the environment that launches YAAS. You can
change the variable name using `api_key_env`. The key is required during source
initialization; changing it requires restarting YAAS. Keep the key out of
source JSON and version control. The runtime passes it in `RCLONE_HEADER`, not
command arguments, and redacts it from rclone transfer diagnostics.

## Protocol assessment

Reviewed [VRCD commit 58a60f6](https://github.com/DeliciousMeatPop/VRCD/tree/58a60f6358cb3f168da7a06987e79f4ed7df0708):

- `src/main/services/gameService.ts` reads `baseUri` and `password`, downloads
  `meta.7z`, base64-decodes the password, and extracts the metadata catalog.
  A root file ending in `GameList.txt` provides semicolon-separated columns
  including Game Name, Release Name, Package Name, Version Code, Last Updated,
  and Size (MB). Additional columns are compatible with YAAS's parser.
- `src/main/services/download/downloadProcessor.ts` addresses each release at
  the lowercase hexadecimal MD5 of its exact release name plus a line feed,
  with a trailing slash for the HTTP directory listing. Releases contain a
  7z archive or split volumes beginning with `.7z.001`.
- `src/main/services/apiKey.ts` supplies a build-time obfuscated key;
  `download/downloadProxy.ts` injects `X-API-Key` using `RCLONE_HEADER`.
  Obfuscation is reversible and does not protect a distributed application key.
- VRCD disables certificate validation for these rclone transfers. YAAS keeps
  certificate validation enabled and uses runtime credentials instead.

YAAS exposes bandwidth limits, progress, and cancellation for this layout.
Catalog refreshes and downloads use temporary directories; failed refreshes
retain the previous release index and failed extraction preserves an existing
download. Successful downloads publish extracted files for YAAS's existing
metadata and installation pipeline. Archive parts are removed with staging.

This backend covers the public endpoint. Mirror selection, donations, provider
metadata images, nested archive processing, and byte-level resume across
cancelled jobs are not implemented. It accepts exactly one main archive per
release. Real provider compatibility still requires testing with the provider
JSON and API key; local fixtures cannot establish access, quotas, redirect
behavior, or the complete range of provider archive layouts. The archive
password uses the existing 7-Zip command-argument mechanism.
