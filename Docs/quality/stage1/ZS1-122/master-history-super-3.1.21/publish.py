from pathlib import Path
import hashlib,json,re,shutil,tarfile
R=Path(__file__).resolve().parents[3];D=Path(__file__).resolve().parent
Q=R/'Docs/quality/stage1/ZS1-122/master-history-super-3.1.21'
assert not Q.exists()
def meta(p):
 b=p.read_bytes();return dict(bytes=len(b),sha256=hashlib.sha256(b).hexdigest())
after=json.loads((D/'root-after-result.json').read_text());assert after['passed']
for p,m in after['owned'].items():assert meta(R/p)==m
pty=json.loads((D/'root-pty-evidence/result.json').read_text())
assert pty['binary']==after['binary'] and len(pty['checks'])==18 and all(x['pass_'] for x in pty['checks'])
assert not pty['http_requests'] and pty['error'] is None and pty['child']['reaped'] and pty['child']['exit_code']==0 and pty['keeper']['exit_code']==0
assert json.loads((D/'root-pty.run.json').read_text())['exit_code']==0
assert '2 passed; 1 failed' in (D/'root-before.log').read_text()
counts=list(map(int,re.findall(r'test result: ok\. (\d+) passed;', (D/'root-after-regression.log').read_text())))
assert counts==[16,22,54,22,14] and sum(counts)==128
offline=json.loads((D/'root-offline/stdout.json').read_text());assert offline['passed']==922 and offline['failed']==0
review='''# ZS1-122 history search Super 修复 · 主控独立整合

Ctrl-R进入历史搜索后原逻辑绕过普通编辑器的修饰键过滤，Super/Super+Shift字符会插入查询，改变当前预览。主控在已经包含会话列表F2修复的主库645936源码上，仅加入worker三个测试复现：2通过/1失败，24组合中的12种Press/Repeat路径发生查询/预览变化；原日志、before源码和测试二进制保留。随后仅把history_search_key的Char分支排除集合增加SUPER，不改Ctrl-R循环、Enter接受、Esc恢复或普通Unicode/Shift输入。

主控完整读取6231B worker补丁、完整README、新增133行测试、相关history函数/实际handle_event→handle_key路径、saved_draft与TestBackend投影helper，以及vendored CSI-u dispatch/modifier解析上下文。输入矩阵真实覆盖两种入口、两种Super组合、ASCII/Unicode与Press/Repeat/Release；handle_event只处理Press与直接handle_key的Repeat差异原样保留。正向断言既检查实际输入也检查二次Enter才Submit；Esc核原fold/cursor。worker早期宽字符padding夹具失败及修正前后原始版本保留，不把夹具调整当产品修复。主控采用的是worker最终相同测试，before/after之间不更改断言。

src/tui.rs仅将guard从当前645936基线增量迁移为7c0d9e5c2cb5f351412aa6e98ac233f5c55580ee1885a431653d8c91b908ba4b，669057B；tests/tui_composer.rs58353B/15ed997ca5e80f1cc0dfa372c463b43249e2acef58d02b7bcd1270f7943874db。会话列表F2、125计时/footer、123补全、BentoBox与顶部加号直开工作目录选择均保留，没有用worker整文件覆盖主库。所有202构建输入在每条管道前后核同身份，只允许本两owned路径的已记录差异。

原生stable-aarch64-apple-darwin、locked/offline：5个相关测试目标共128个不同Rust测试通过，其中composer54、project-workspace14、BentoBox16、interaction22、palette22；fmt、strict Clippy lib+composer和debug build均exit0。该128与先前F2的143存在重叠，不合并宣称271个独立测试。vendor原unused_parens警告保留。新debug50751592B，SHA256 f455ee073c57d9428149b42def234815028a7eb7e0f3d9bc971373b99c0c1a15。

worker的唯一真实PTY13项行为检查通过，但在child正常退出后tcgetattr(slave)收到macOS ENOTTY，harness exit1，result.json缺失，terminal flags未证实、最终HTTP计数null。438payload包、原script、输入hex、raw、checkpoints、错误和事后观察全部保留。主控完整读纯离线verifier后在新的副本只运行一次922项完整性审计，0失败；它不执行产品，也不将这个旧PTY提升为通过。

主控随后完整静态审阅新PTY脚本与24行keeper，针对上述观测生命周期问题做独立实际验证：keeper持有controlling PTY，恰好启动一个新的真实zenpi进程，等待并reap它，报告exit后保持TTY直到主控读取恢复flags，再退出并被主控reap。没有产品重试或旧runner重跑，没有改产品终端恢复实现。新固定输入仍使用真实CSI-u Super9/ShiftSuper10字节与普通后续sentinel，非KeyEvent注入。主控只执行一次新用例，18项全部通过、driver0/zenpi0/keeper0。核对Unicode和Shift输入、两种Super不改查询、后续普通字符继续处理、Ctrl-R旧匹配、Esc原fold/cursor持久恢复、Enter只接受而不提交、退出/回收、ICANON/ECHO/ISIG flags与原值一致、LeaveAlternateScreen、最终0HTTP请求及服务器停止。local flags完整观察值1483→1483，mask392；子PID51828与keeper51827均正常reap，之后ps已确认不存在。

真实checkpoint与终端raw在root-pty-evidence，最终server请求列表[]是主控这次完整运行实测，不能倒灌成worker失败运行的最终统计。主控流程只发本地slash控制命令，session journal无model turn；没有生产release/冷启动预算、117/131/FIFO/DTrace/外部真实模型调用。输入编码证明当前vendored reader支持这些序列，不证明每种物理终端都会把OS快捷键转发给程序。

仅接受122这处有界产品修复，完整122及G-CODE/G-HOST整个交互矩阵仍待验；阶段50/121不因局部修复增加。095完整文件已独立接受，096/072及其余文件/目录候选继续逐个主控复核。回退只撤本root.patch的guard和三个测试，先核当前差异，不恢复整个TUI或删除后来工作。旧F1/F2/F3发现和失败均保留，F3无delta终态目前仍未证明实际可达，不声称修复。
'''
(D/'review.md').write_text(review)
verification=dict(after,item='ZS1-122',scope='local history Super guard',master_verified=True,whole_item_accepted=False,rust_tests=128,new_tests=3,before_failed_tests=1,root_pty_checks=18,root_pty_runs=1,root_pty_pass=True,root_http_count=0,worker_pty_harness_exit=1,worker_final_http_count=None,offline_checks=922)
(D/'master-verification.json').write_text(json.dumps(verification,indent=2)+'\n')
Q.mkdir(parents=True)
for n in ['review.md','master-verification.json','root.patch','root-before-inputs.json','root-after-inputs.json','root-before.log','root-before.run.json','root-after-regression.log','root-after-regression.run.json','root-after-format.log','root-after-format.run.json','root-after-clippy.log','root-after-clippy.run.json','root-after-build.log','root-after-build.run.json','root-pty.stdout.log','root-pty.stderr.log','root-pty.run.json','root-pty-static-review.json','root_pty_history_super.py','pty_keeper.py','root-after.py','publish.py']:
 shutil.copyfile(D/n,Q/n)
shutil.copyfile(D/'worker/manifest.json',Q/'worker-manifest.json')
shutil.copyfile(D/'worker/README.md',Q/'worker-report.md')
shutil.copyfile(D/'root-pty-evidence/result.json',Q/'pty-result.json')
with tarfile.open(Q/'integration-evidence.tar.gz','x:gz') as archive:
 for p in sorted(D.rglob('*')):
  if p.is_file():archive.add(p,arcname=str(p.relative_to(D)),recursive=False)
(Q/'manifest.json').write_text(json.dumps(dict(item='ZS1-122',scope='local-repair',artifacts={str(p.relative_to(Q)):meta(p) for p in Q.rglob('*') if p.is_file()}),indent=2)+'\n')
print('Published',str(Q.relative_to(R)),meta(Q/'manifest.json'))
