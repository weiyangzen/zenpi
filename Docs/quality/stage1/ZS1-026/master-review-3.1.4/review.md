# ZS1-026 master review


## Independent controller whole-file review

Controller read all323 lines/11550 bytes and the complete worker report. Schema, default operations, definition factory, every execute branch, renderer spread and createGrepTool wrapper are covered. Imported renderers/ensureTool/path helpers/truncate/wrapper remain context-only; neither source code nor source tests were executed.

Precise differences: source defaults regex and hidden; target retains literal default and explicit hidden. Source directory paths are relative to searchPath and single-file results use basename; target returns workspace-relative paths. Source context reading normalizes CRLF/CR to newline, whereas direct rg text removes CR. The source close callback removes abort listeners before awaiting context formatting, so late cancellation during formatting is not guaranteed; _onUpdate is unused. An async close formatting rejection is not uniformly caught by the outer try; settle idempotence alone does not imply complete error containment. Source stderr/file cache lack separate byte caps; target bounds both traversal and output.

Target search.rs all566 lines and candidate tests453 lines were independently reviewed. tools.rs advanced options call actual installed rg. core.prepare_tool now persists cumulative Processes reservations before every search dispatch (literal0/find2/regex+glob maximum19); it does not treat an active slot as a reusable start budget. New real HTTP Agent/headless tests cover precise results in the continuation, insufficient budget with zero spawn, journal snapshot before first child, usage retained after reopen and cancel/reap. First cancellation fixture failed because a PID marker existed before its contents were written; trigger now requires a parsed PID, without extending sleeps or changing product cancellation.

Only026 file-understanding is accepted. Product112, directory053 and other source files still need independent gates. Current immutable Blueprint grants explicitly deny advanced search lacking confined traversal authorization; original literal search remains gate-aware.
