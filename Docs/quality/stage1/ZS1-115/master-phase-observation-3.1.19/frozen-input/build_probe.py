from pathlib import Path
import hashlib,json,subprocess,datetime
root=Path.cwd();r=root/'.ops/ux130-phase-observation';source=(root/'src/extension_runtime.rs').read_text();base=(r/'baseline/extension_runtime.rs').read_text();h=lambda data:hashlib.sha256(data).hexdigest()
phase=source[source.index('    fn fixture_phase('):source.index('    struct FixturePids(')]
pids=source[source.index('    struct FixturePids('):source.index('    fn isolated_case(')]
a=source.index('        let start = Instant::now();',source.index('    fn isolated_case('));b=source.index('\n    #[test]',a);tail=source[a:b];baseline_a=base.index('        let start = Instant::now();',base.index('    fn isolated_case('));baseline_b=base.index('\n    #[test]',baseline_a);assert tail==base[baseline_a:baseline_b]
preamble='''use std::{fs, path::{Path, PathBuf}, process::{Command, Stdio}, time::{Duration, Instant}};
extern crate libc;
'''
head='''fn observe(ext: &Path, mode: &str, preserve: bool) -> bool {
    let _phase_log = preserve.then(|| FixturePhaseLog(ext.to_owned()));
    let api = 0;
    let pipe = "diagnostic";
    let stop = mode;
    let cleanup = FixturePids(ext.to_owned());
    let mut helper = Command::new(std::env::current_exe().unwrap())
        .arg("--child").arg(ext).arg(mode)
        .stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
'''
main=r'''
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let ext = PathBuf::from(&args[2]);
    if args[1] == "--child" {
        fixture_phase(&ext.join("phases.log"), "helper-entry");
        if args[3] == "watchdog" {
            fixture_phase(&ext.join("phases.log"), "intentional-watchdog-stall");
            std::thread::sleep(Duration::from_secs(10));
        }
        std::process::exit(if args[3] == "nonzero" { 17 } else { 0 });
    }
    if args[1] == "--snapshot" {
        let _phase_log = FixturePhaseLog(ext);
        return;
    }
    if args[1] == "--unwind" {
        let outcome = std::panic::catch_unwind(|| {
            let _phase_log = FixturePhaseLog(ext.clone());
            fixture_phase(&ext.join("phases.log"), "before-intentional-unwind");
            panic!("intentional fixture failure stays a failure");
        });
        fs::remove_dir_all(ext).unwrap();
        std::process::exit(if outcome.is_err() { 1 } else { 0 });
    }
    let success = observe(&ext, &args[3], args[1] == "--after");
    println!("observed_fixture_success={success}");
    println!("phase_file_before_cleanup_bytes={}", fs::metadata(ext.join("phases.log")).map(|m| m.len()).unwrap_or(0));
    fs::remove_dir_all(ext).unwrap();
    std::process::exit(if success { 0 } else { 1 });
}
'''
probe=preamble+phase+pids+head+tail+main;(r/'phase_probe.rs').write_text(probe)
libc=root/'target/debug/deps/liblibc-639f435d40576036.rlib';argv=['rustc','+stable-aarch64-apple-darwin','--edition=2024','-D','warnings',str(r/'phase_probe.rs'),'--extern','libc='+str(libc),'-o',str(r/'phase_probe')];record=dict(argv=argv,cwd=str(root),started=datetime.datetime.now(datetime.timezone.utc).isoformat(),candidate_source_sha256=h(source.encode()),baseline_source_sha256=h(base.encode()),watchdog_tail_byte_identical=True,extracted_phase_sha256=h(phase.encode()),extracted_watchdog_sha256=h(tail.encode()),libc_sha256=h(libc.read_bytes()),probe_source_sha256=h(probe.encode()))
with (r/'build.log').open('w') as out:record['exit']=subprocess.run(argv,cwd=root,stdout=out,stderr=subprocess.STDOUT).returncode
record['ended']=datetime.datetime.now(datetime.timezone.utc).isoformat();record['log_sha256']=h((r/'build.log').read_bytes());record['probe_sha256']=h((r/'phase_probe').read_bytes()) if record['exit']==0 else None;(r/'build.run.json').write_text(json.dumps(record,indent=2)+'\n');print(record['exit']);assert record['exit']==0
