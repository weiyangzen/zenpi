# ZS1-053 directory packet

Provisional, [_], master review pending. See current_folder_learn.md for integration findings and fixture limits; directory-scope.json records actual immediate children and dependency acceptance. Old packets were reused without mutation or repeated test counts.

Replay from a writable copy using Node >=22.19.0:

    npm ci --prefix runtime --ignore-scripts --no-audit --no-fund
    NODE_BIN=/absolute/path/to/node sh run.sh

run.sh verifies source hashes and writes fresh replay.* logs. Real pinned Vitest is used; node_modules is not shipped. Source copies match upstream. Scripted responses and fixture ancestry are not provider or journal evidence. manifest.json hashes every frozen regular file except itself.

Native replay requires Darwin arm64 and the exact rg binary in native-tools.json. Set RG_BIN to its path; the runner checks its hash and copies it into a fresh isolated agent directory. Frozen fd10.5.0 is restored from its gzip with licenses. Tool execution is offline. Actual shell/files/subprocesses are used; run.sh leaves owned temporary fixtures for inspection. Renderer-only bridges do not validate UI.
