from pathlib import Path
import datetime,hashlib,json,os,shutil,signal,subprocess
R=Path(__file__).resolve().parents[2];D=R/'.ops/stage1_execution/blueprint-version-current-3.1.21';D.mkdir(exist_ok=False)
def meta(p):
 b=p.read_bytes();return dict(bytes=len(b),sha256=hashlib.sha256(b).hexdigest())
def dump(p,x):p.write_text(json.dumps(x,ensure_ascii=False,indent=2)+'\n')
def run(name,args,allowed=(0,),timeout=300):
 env=os.environ.copy();env.update(CARGO_NET_OFFLINE='true',CARGO_BUILD_JOBS='2',PYTHONDONTWRITEBYTECODE='1');env.pop('ZENPI_DOMAIN_STORE',None)
 start=datetime.datetime.now(datetime.timezone.utc).isoformat();timed=False
 with (D/(name+'.log')).open('xb') as f:
  p=subprocess.Popen(args,cwd=R,env=env,stdout=f,stderr=subprocess.STDOUT,start_new_session=True)
  try:code=p.wait(timeout=timeout)
  except subprocess.TimeoutExpired:timed=True;os.killpg(p.pid,signal.SIGKILL);p.wait(timeout=10);code=124
 dump(D/(name+'.run.json'),dict(argv=args,cwd=str(R),started_at=start,ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),exit_code=code,pid=p.pid,reaped=p.poll() is not None,timed_out=timed,domain_store_env_unset_for_test_isolation=True,log=meta(D/(name+'.log'))));print(name,code,flush=True);assert code in allowed,name
W=Path('/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/stage132-blueprint-version321-ready');assert meta(W/'manifest.json')['sha256']=='fb76f66bcc90b0cf7ec9b1c61b56306a2da8866210e434bb920f8e0d1ee7d9a4'
shutil.copytree(W,D/'worker');W=D/'worker';run('worker-offline',['python3','-B',str(W/'verify.py')])
keys=json.loads((R/'.ops/stage1_execution/shutdown-current-3.1.21/inputs-after.json').read_text());before={r:meta(R/r) for r in keys};dump(D/'inputs-before.json',before)
for rel in before:
 p=D/'before'/rel;p.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(R/rel,p)
assert (R/'src/headless.rs').read_bytes()==(W/'evidence/build-input/src/headless.rs').read_bytes()
regression='''
#[test]
fn goal_create_respects_explicit_version_and_does_not_mutate_on_rejection() {
    for (target, expected_version) in [
        ("single@missing", None),
        ("single@x@y", None),
        ("single", Some("1")),
        ("multi", None),
        ("multi@2", Some("2")),
        ("label@x@y", Some("x@y")),
    ] {
        let dir = tempdir().unwrap();
        let session_path = dir.path().join("session.jsonl");
        let domain_path = path_for_session(&session_path);
        assert!(domain_path.starts_with(dir.path()), "fixture store escaped tempdir");
        let mut store = DomainStore::open(&domain_path).unwrap();
        for (id, version) in [("single", "1"), ("multi", "1"), ("multi", "2"), ("label", "x@y")] {
            store.put_blueprint(Blueprint::new(id, version, vec![BlueprintItem::new("inspect", 1)]).unwrap()).unwrap();
        }
        drop(store);
        let before = std::fs::read(&domain_path).unwrap();
        let mut agent = Agent::with_echo(SessionStore::open(&session_path).unwrap());
        let input = [
            serde_json::json!({"type":"command","id":"create","text":format!("/goal create selected {target}")}),
            serde_json::json!({"type":"shutdown","id":"stop"}),
        ].into_iter().map(|value| serde_json::to_string(&value).unwrap()).collect::<Vec<_>>().join("\\n") + "\\n";
        let mut output = Vec::new();
        run_headless(&mut agent, Cursor::new(input.into_bytes()), &mut output).unwrap();
        let wire = records(&output);
        let response = wire.iter().find(|row| row["id"] == "create" && row["type"] == "response").unwrap();
        let restored = DomainStore::open_read_only(&domain_path).unwrap();
        if let Some(version) = expected_version {
            assert_eq!(response["success"], true, "{target}: {response}");
            let goal = restored.goal("selected").expect("goal must be durable");
            let blueprint_id = target.split('@').next().unwrap();
            let blueprint = restored.blueprint(blueprint_id, version).unwrap();
            assert_eq!(goal.blueprint_id, blueprint_id);
            assert_eq!(goal.blueprint_version, version);
            assert_eq!(goal.blueprint_digest, blueprint.digest);
            assert_eq!(goal.status, GoalStatus::Queued);
            assert_eq!(response["data"]["zenpi_started"], false);
        } else {
            assert_eq!(response["success"], false, "{target}: {response}");
            assert_eq!(response["code"], "blueprint_not_found");
            assert!(restored.goal("selected").is_none());
            assert_eq!(std::fs::read(&domain_path).unwrap(), before, "{target} mutated domain store");
        }
        assert!(agent.history().iter().all(|turn| turn.role != TurnRole::User && turn.role != TurnRole::Assistant));
    }
}
'''
p=R/'tests/headless_domain_owner.rs';assert regression.splitlines()[2] not in p.read_text();p.write_text(p.read_text()+regression)
run('format-test',['rustfmt','+stable-aarch64-apple-darwin','--edition','2024',str(p)])
run('regression-before',['cargo','+stable-aarch64-apple-darwin','test','--offline','--locked','--jobs','2','--test','headless_domain_owner','goal_create_respects_explicit_version_and_does_not_mutate_on_rejection','--','--exact','--nocapture'],allowed=(101,))
assert 'test result: FAILED. 0 passed; 1 failed' in (D/'regression-before.log').read_text()
run('apply-check',['git','apply','--check',str(W/'candidate.patch')]);run('apply',['git','apply',str(W/'candidate.patch')])
after={r:meta(R/r) for r in before};assert {r for r in before if before[r]!=after[r]}=={'src/headless.rs','tests/headless_domain_owner.rs'};assert (R/'src/headless.rs').read_bytes()==(W/'files/src/headless.rs').read_bytes();dump(D/'inputs-after.json',after)
run('root-tests',['cargo','+stable-aarch64-apple-darwin','test','--offline','--locked','--jobs','2','--test','headless_domain_owner','--test','headless_protocol','--test','domain_store','--test','tui_goal_owner','--test','domain_execution_owner'])
run('root-clippy',['cargo','+stable-aarch64-apple-darwin','clippy','--offline','--locked','--jobs','2','--all-targets','--','-D','warnings'])
run('root-fmt',['cargo','+stable-aarch64-apple-darwin','fmt','--check'])
assert all(meta(R/r)==m for r,m in after.items()),'source drift'
print('Version selection and durable regression integrated; fresh production validation pending.',flush=True)
