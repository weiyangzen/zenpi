# ZS1-058 directory packet

Provisional, [_], master review pending. See current_folder_learn.md for integration findings and fixture limits; directory-scope.json records actual immediate children and dependency acceptance. Old packets were reused without mutation or repeated test counts.

Replay from a writable copy using Node >=22.19.0:

    npm ci --prefix runtime --ignore-scripts --no-audit --no-fund
    NODE_BIN=/absolute/path/to/node sh run.sh

run.sh verifies source hashes and writes fresh replay.* logs. Real pinned Vitest is used; node_modules is not shipped. Source copies match upstream. Scripted responses and fixture ancestry are not provider or journal evidence. manifest.json hashes every frozen regular file except itself.

This packet only collects the23 original cases; it does not rerun them. Their historical actual execution and per-case assertions are preserved under reuse/. run.sh performs collection only.
