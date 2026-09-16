# Receipt packaging correction

The first actual acceptance attempt exited 1 after reaching worker-self-tested state: the receipt referenced empty historical files directly, while the checker requires nonempty top-level artifacts. The failure and original receipts are preserved here. No source, report, historical payload, validator rule or test result was changed.

The corrected receipts retain all nonempty evidence and reference the complete worker archive, its manifest and an explicit index of the preserved empty files. Empty patch-base placeholders and empty syntax-output files remain in place with their original sizes and hashes; none is promoted to standalone semantic proof. The unique full report remains a direct nonempty artifact. This is a receipt packaging correction, not a new behavioral pass. Only ZS1-089 can be promoted after its actual gate succeeds.
