# Transform Note: `b3ehive/looper-cron-builder/references/b3ehive-bridge-contract.md`

source_path: `b3ehive/looper-cron-builder/references/b3ehive-bridge-contract.md`
source_hash: `b0d49aeaf50bac18546b1eff8178887360e8e2211cb4c590db5ba27a57362869`

The source separates resource envelopes, leases, side-effect gates, route and
estimate decisions, evidence, nested attribution, and LooperLog feedback.
zenpi keeps those as bounded inert serde records in `b3.rs`. It does not port
the cron scheduler, worker runners, reward daemon, or nested-agent execution;
the embedding host retains those responsibilities. Unit tests assert budget,
expiry, gate, digest, and Master-authority behavior.
