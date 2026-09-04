# Release Contract

`tools/release.sh` builds a locked production binary without `dev-fixtures`,
stages the license and multilingual README, emits a deterministic CycloneDX
package inventory, archives the result, and writes a SHA-256 sidecar. The
release workflow runs native jobs for macOS arm64/x86_64, Linux arm64/x86_64,
and Windows x86_64. Each job executes the packaged binary before upload.

Local verification:

```bash
rm -rf dist
tools/release.sh
(cd dist && shasum -a 256 -c *.sha256)
tar -tzf dist/*.tar.gz
dist/*/zenpi --help
```

The production package must not contain `.codex`, `.zenpi`, `auth.json`, test
fixtures, or environment files. Installing or upgrading replaces only the
binary; sessions and profiles remain under `~/.zenpi/` and are never included
in an archive. A failed smoke or checksum step prevents publication.

## Acceptance Receipt (2026-09-04)

- Host package: `aarch64-apple-darwin`; archive checksum verified and packaged
  `zenpi --help` executed successfully.
- Archive inventory: binary, multilingual README, license, and CycloneDX SBOM
  only; the credential/fixture path scan returned no matches.
- Installed release smoke: isolated Cargo install, Responses streaming fixture,
  cross-process session resume, PTY resize, in-flight cancellation, and terminal
  restoration passed.
- Real imported profile: `config doctor --profile codex --json` reported a
  Responses profile and credential presence without revealing the credential.
  A real provider request returned `ZENPI_REAL_FRAMEWORK_OK` earlier in the same
  acceptance run. A later repeat reached the configured gateway but received
  its explicit usage-limit error; zenpi surfaced that provider error and did not
  substitute echo. No credential value is recorded in this receipt.
