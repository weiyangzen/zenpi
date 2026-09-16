use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use std::{thread, time::Duration};
use zenpi::{approval::ApprovalRequest, tools::{ToolOrigin, ToolSideEffect}, tui::TuiState};
fn request(id: &str) -> ApprovalRequest {
    ApprovalRequest { request_id: id.into(), turn_id: "turn-status".into(), call_id: format!("call-{id}"), tool: "write_file".into(), side_effect: ToolSideEffect::WorkspaceWrite, arguments: serde_json::json!({"path":"unused.txt"}), preview: None, origin: ToolOrigin::AgentTool, policy_digest: None, lease_id: None }
}
fn header(s: &mut TuiState) -> (u64, String) {
    let mut t = Terminal::new(TestBackend::new(180, 35)).unwrap();
    t.draw(|f| s.render_bentobox(f, "zenpi")).unwrap();
    let row = (0..180).map(|x| t.backend().buffer()[(x,1)].symbol()).collect::<String>();
    let end = row.find("s · Ctrl-C)").expect("visible elapsed header");
    let prefix = &row[..end];
    let start = prefix.rfind('(').expect("elapsed opening delimiter") + 1;
    (prefix[start..].parse().expect("seconds"), row)
}
fn dismiss(s: &mut TuiState) { s.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)); }
fn observe(name: &str, s: &mut TuiState, expect_running: bool) -> serde_json::Value {
    let (before, before_header) = header(s);
    thread::sleep(Duration::from_millis(1150));
    let (after, after_header) = header(s);
    serde_json::json!({"name":name,"before_seconds":before,"after_seconds":after,"pending_approvals":s.approval_count(),"expected_running":expect_running,"passed":if expect_running {after > before} else {after == before},"before_header":before_header,"after_header":after_header})
}
fn main() {
    let mut results = Vec::new();
    let mut pending = TuiState::default();
    pending.set_busy(true);pending.present_approval(request("pending"));pending.set_status("Approval required");dismiss(&mut pending);
    pending.set_status("Draft kept");
    results.push(observe("pending_approval_survives_unrelated_status", &mut pending, false));
    let mut retired = TuiState::default();
    retired.set_busy(true);retired.present_approval(request("retired"));retired.set_status("Approval required");dismiss(&mut retired);retired.retire_approval("retired");
    results.push(observe("last_approval_retirement_resumes_without_tool_event", &mut retired, true));
    let mut ordered = TuiState::default();
    ordered.present_approval(request("ordered"));dismiss(&mut ordered);ordered.set_busy(true);
    results.push(observe("approval_before_busy_stays_paused", &mut ordered, false));
    let mut partial = TuiState::default();
    partial.set_busy(true);partial.present_approval(request("first"));partial.present_approval(request("second"));partial.set_status("Approval required");dismiss(&mut partial);partial.retire_approval("first");
    results.push(observe("other_approval_still_waiting_control", &mut partial, false));
    let failed = results.iter().filter(|x| x["passed"] == false).count();
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({"kind":"public-state-component-observation","provider_requests":0,"actual_pty":false,"cases":results,"failed":failed})).unwrap());
    if failed != 0 {std::process::exit(1)}
}
