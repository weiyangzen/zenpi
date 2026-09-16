# codex-rs/tui/src/bottom_pane/pending_thread_approvals.rs — whole-file candidate review

Status: [_] master acceptance pending; ZS1-304.
Source root: `/Users/wangweiyang/GitHub/codex`
Reference HEAD: `b3b3d262787f4902a7449f17d793241a34d311ad`
Source bytes: 4105
SHA256: `a401d7c6ba3051fcf2c20152a9e86a3f4f8751a87470cbc7f518001326618a2f` (matches frozen blueprint).
Read scope: 147/147 lines including3 inline tests and their embedded snapshots. Single chunk [0,4105), <=256KiB, chunk hash equals file hash.

This is a notification widget for approvals in inactive tasks, not an approval owner. It stores display names, avoids dirty changes when a new list equals the old list, and renders nothing for empty state or widths below4. Rendering adapts line wraps with a colored alert prefix, shows at most3 names and an overflow indication, then a discoverable command to switch tasks. desired_height delegates to the same renderable, keeping measurement and output aligned. The input vector itself is not capped here; the caller must bound retained state.

Three source tests verify zero height for empty state, one task and the3-item cap with a fourth task in embedded snapshots. They were read completely, not executed. No allow/deny/cancel or authority checks occur in this file, and snapshot rendering must not be mistaken for permission enforcement.

Zenpi124 mapping: show inactive project approval attention next to canonical project identity and a working switch affordance. A tab notification cannot resolve another project's call. The existing runtime coordinator must retain project/call ownership, timeout and decisions; switching to the source project restores approval focus without losing the draft. Limit visible notification counts while preserving all queued requests in the bounded owner. Do not copy Codex's task/agent command label into Zenpi's project control surface.
