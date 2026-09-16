# ZS1-016 source behavior [_]

23 cases against unchanged complete source, actual pinned SDK and actual loopback HTTP. No mocked imports; source-native-final.json/log passing, source-native.json/log initial incorrect same-model ID expectation retained. Target-native.json/log: 17 target tests. Prior full report reused by matching original frozen manifest hash. The source parser repairs some JSON; native target intentionally rejects it. Toolcall_end is not a provider terminal.

Reproduce: npm ci --prefix .ops/source016-runtime --ignore-scripts --no-audit --no-fund (use retained package-lock and matching package.json), then the exact NODE_PATH/Bun argv in receipt. Runtime installation leaves source tree and HOME/CODEX_HOME unchanged. No paid provider, external endpoint or user credential was used. No upstream-suite or all-platform cancellation claim.
