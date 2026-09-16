# ZS1-122 — history search Super modifier fix candidate

Status `[_]`: only F1 from the current ZS1-085 review. Master migration and independent acceptance pending; this is not whole ZS1-122 acceptance. F2 SessionList mouse mapping and F3 no-delta canonical terminal are untouched.

Entering history search with Ctrl-R bypasses the ordinary composer's modifier filter. Previously `history_search_key` excluded Ctrl/Alt but accepted Super and Super+Shift characters, changing the query and selected history preview. The change adds `KeyModifiers::SUPER` to that one character guard. Plain Unicode, Shift characters, Ctrl-R history cycling, Enter selection and Esc restoration keep their existing paths. No broad modifier dispatch or repeat-event semantics were changed.

## Input and ownership

- Requirement3.1.21 digest `3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d`; run `zenpi-stage1-20260911`; master directly assigned this bounded122 follow-up after085 completed.
- Read-only main source baseline:668438 bytes /SHA `e45131dcf190d55068e369e55b8a3cc0dda18234ccee2c2ca335c9cefdd15904`. Private source after:668457 bytes /SHA `24462ac5fed29cd4605dea7e8643bf03e44be78f0fdac824905da3c2c362d44a`.
- Test baseline:52941 bytes /SHA `30d9b19bc250409bac1e4254b7237f11cf8de625913d55b669a044665e89d7c1`. After:58353 bytes /SHA `15ed997ca5e80f1cc0dfa372c463b43249e2acef58d02b7bcd1270f7943874db`.
- Only `src/tui.rs` and `tests/tui_composer.rs` differ from the complete captured private project inputs. `candidate.patch` is6231 bytes. `before/`, `owned-files.json`, `input-manifest.json` and `scope-observation.json` bind exact bytes. Files were copied by writing bytes with fresh mtimes; native Cargo used a new private target directory.
- Main checkout, blueprint, claims, master receipts, old ready packages, HOME and CODEX_HOME were not modified. ZENPI_HOME is a new private directory for the one PTY child. No subagent/new task was created; no old runner, full-lib test, diff pidfile race,117/131/FIFO/DTrace/release/startup-budget/prewarm work was run.

## Reading chain and semantic review

`reading-chain/zs1-085/` preserves the entire frozen085 packet including its original contiguous full-source ledger and two separately read increments through e451. Its manifest remains `e2b0073c5667e58e20021e4078666eaa5225a2fbc5eb391c4ee3c3ce57d2a714`. The new baseline is byte-identical to that reviewed final source; this task reread the history implementation, display, true event route and exact new delta. It does not replace full-source reading with a hash.

The previously bounded composer test reading was completed across all missing ranges146–669,781–1030,1205–1248; combined with085 ranges this covers all1406 baseline lines. These include scheduled/FIFO receipt ownership, ordinary paste/IME/Enter guard, fold/grapheme/budget, kill/yank, history/project recovery and external editor cases. `reading.json` records scope. All additions and the final implementation hunk were reviewed. Vendored crossterm CSI-u dispatch, modifier mask and parser tests were read as bounded protocol context; vendor is unchanged.

The new matrix exercises both `TuiState::handle_event(Event::Key(...))` and direct `handle_key`:2 entry routes ×2 modifiers ×2 ASCII/Unicode characters ×3 Press/Repeat/Release kinds=24 cases. Each starts from an older selected `alpha` history match with an unsent1200-character folded draft, and checks query text, preview/cursor and persisted original draft. Existing event-layer Repeat handling differs from direct handle_key; the fix does not alter that contract.

Positive controls drive real event dispatch for Unicode+Shift query `界A`, Ctrl-R older match, Enter accepting into the composer without submitting, and a second deliberate Enter returning Submit. A separate Esc test verifies exact original fold/cursor restoration after an ignored shortcut. The whole existing normal composer Super matrix remains in the54-test target.

## Validation and preserved failures

Every Cargo invocation used native `stable-aarch64-apple-darwin`, locked/offline with an independent target (except fmt, which performs formatting inspection). Full argv/cwd/source/test hashes, timestamps, exits and stdout/stderr hashes are in `logs/*.receipt.json`.

| Observation | Result | Evidence |
|---|---|---|
| Initial unchanged product with new tests v1 |1PASS/2FAIL, exit101 |`before-history-super.*`; one true Super regression and one test display assertion issue |
| Corrected test v2, product still unchanged |2PASS/1FAIL, exit101 |`before-history-super-v2.*`;12 of24 combinations expose query mutation |
| Fixed product, complete composer target |54PASS/0FAIL, exit0 |`after-composer.*`; includes3 new and51 existing tests |
| Native dev build `--bin zenpi` |exit0 |`native-build.*`; no release build or startup measurements |
| fmt check |exit0 |`fmt-check.*` |
| strict Clippy `--lib --test tui_composer -- -D warnings` |exit0 |`clippy.*`; pre-existing vendored unused_parens warning retained, no vendor edit |
| One fresh production PTY |13 behavioral checks recorded true; **harness exit1** during post-exit cleanup observation |`pty-evidence/`, `logs/pty.*`, `pty-postmortem-observation.json` |

The initial positive Unicode assertion concatenated TestBackend cells and forgot the padding cell after a wide glyph. It was corrected to compare the history title with whitespace removed; actual input `界A new`, cycling, acceptance and original draft equality remain exact. `attempts/test-v1/` preserves that test input, and both initial log files are immutable. v2 uses the same final tests as the passing after run, so the single implementation guard is the causal change. Failure messages now print bounded prefixes rather than repeating the entire fixture draft; original verbose failure logs remain intact.

## Real PTY evidence and limitation

The once-run `pty_history_super.py` was fully statically reviewed before execution (`pty-static-review.json`). It launches the actual native production binary in an owned controlling PTY, not an Echo callback or injected KeyEvent harness. Binary `bin/zenpi`: Mach-O arm64,50811032 bytes,SHA `a9aaf3853bcef86bb7d466473039b2d9f147e3188b665e2c17fb122216a33260`.

The captured crossterm parser dispatches numeric CSI-u to its keyboard decoder and interprets modifier mask minus1:8=Super,9=Shift|Super. Actual bytes `ESC[120;9u` and `ESC[88;10u` were sent through the PTY. `input-bytes.json` preserves all sent hex and timings; `terminal.raw` and per-check screens preserve output. A following plain `p` changes `help` to `helpp`, proving continued input processing after the two modified sequences. This verifies supported protocol bytes, not that every physical terminal maps the OS Super key the same way.

The13 persisted checks cover startup, control-only history with no HTTP at that checkpoint, exact folded draft/cursor in real project checkpoint, Unicode/Shift input, normal query/latest match, both ignored Super modifiers, following sentinel, Ctrl-R older match, Esc durable restoration, Enter-only acceptance into draft, and normal child exit/reap (`process.wait()==0`). The three checkpoint snapshots independently preserve original/restored/accepted draft states. Raw output includes LeaveAlternateScreen.

After those13 successes, `finally` called `termios.tcgetattr(slave)` after the child had normally exited; macOS returned ENOTTY. The harness therefore exited1 and never emitted its final result.json. This remains a harness failure: terminal flag restoration was **not verified**, and the final HTTP server request list was not persisted. Only the early no-HTTP check is directly recorded. The final journal has header plus extensions_selected, no model/turn records; dispatched commands were only `/help`, `/help input`, `/status`, `/quit`, with no planned model call. These support absence of model work but do not replace the missing final server counter with a claimed measured0.

No second PTY run or retroactive result was manufactured. `pty-postmortem-observation.json` explicitly labels its conclusions as inspection of already-persisted artifacts. Master can decide whether this13-check behavior evidence plus the54-test target is sufficient or run its independent corrected cleanup observation. The worker does not claim the full PTY harness passed.

## Integration and rollback

Apply only `candidate.patch` after checking each before hash in `owned-files.json`. If current main has moved, port the single history guard plus the three new tests onto current source; do not copy the whole private project over main. The085 baseline reading chain remains provenance, not permission to replace subsequent owner work. F2/F3 and all unrelated source are excluded.

Rollback only this hunk and its new tests (reverse the scoped patch when after hashes match), or use the two `before/` snapshots in an isolated comparison. Do not reset/stash/clean or restore whole files over unrelated main edits. No authority status is changed by rollback.

The frozen manifest covers all packet bytes except itself. At most one newly written offline verifier may run after its full static review; its JSON/exit/stderr stay outside the packet. Its pass means package integrity and honest validation records, including the failed PTY harness, not master G-CODE/G-HOST acceptance. Stop after this candidate; further122 work is separately assigned.
