#!/usr/bin/env python3
"""Production release PTY: resource invocation, path completion and reload lifecycle."""
from __future__ import annotations
import argparse,base64,fcntl,hashlib,json,os,struct,termios,threading,time
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
from pathlib import Path
from tempfile import TemporaryDirectory
from tui_project_workspace_smoke import Terminal,screen_text
from tui_user_shell_smoke import drain

def main():
 p=argparse.ArgumentParser();p.add_argument('--binary',required=True,type=Path);p.add_argument('--evidence',type=Path,default=Path('Docs/quality/stage1/ux123-pty-result.json'));args=p.parse_args();binary=args.binary.resolve()
 requests=[];slow=threading.Event();release=threading.Event()
 class Handler(BaseHTTPRequestHandler):
  def log_message(self,*args):pass
  def do_POST(self):
   body=json.loads(self.rfile.read(int(self.headers['Content-Length'])));requests.append(body);number=len(requests)
   latest=next((v for v in reversed(body.get('input',[])) if v.get('role')=='user'),{})
   self.send_response(200);self.send_header('Content-Type','text/event-stream');self.end_headers()
   def emit(kind,**data):self.wfile.write(('event: '+kind+'\ndata: '+json.dumps(dict(type=kind,**data))+'\n\n').encode());self.wfile.flush()
   try:
    emit('response.created',response={'id':f'resp-{number}','model':body['model']})
    if 'slow-resource-case' in json.dumps(latest):
     emit('response.output_text.delta',delta='BUSY_RESOURCE_MARKER');slow.set()
     if not release.wait(20):return
    emit('response.output_text.delta',delta=f'REPLY_{number:02}')
    emit('response.completed',response={'id':f'resp-{number}','model':body['model'],'status':'completed','usage':{'input_tokens':12,'output_tokens':3,'total_tokens':15}})
   except (BrokenPipeError,ConnectionResetError):pass
 server=ThreadingHTTPServer(('127.0.0.1',0),Handler);threading.Thread(target=server.serve_forever,daemon=True).start()
 result={'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'checks':{}};terminals=[]
 try:
  Path('.ops').mkdir(exist_ok=True)
  with TemporaryDirectory(prefix='command-pty-',dir=Path('.ops').resolve()) as raw:
   root=Path(raw)/('工作 é '+('a'*150))/('来源 space '+('b'*150))/('项目 '+('c'*150));work=root/'initial';config=root/'fixture'
   for path in [work/'.zenpi/skills/sample',work/'.zenpi/prompts',work/'工作 folder',root/'sessions',config]:path.mkdir(parents=True)
   skill=work/'.zenpi/skills/sample/SKILL.md'
   def set_skill(version):skill.write_text(f'---\nname: sample\ndescription: Sample skill metadata\ndisable-model-invocation: true\n---\nONLY_SELECTED_SKILL_{version}\n')
   set_skill('ONE');(work/'.zenpi/prompts/fixture-hi.md').write_text('---\ndescription: Greeting fixture\n---\nTEMPLATE_EXPANDED: $1 | $ARGUMENTS\n')
   (work/'工作 folder/note file.txt').write_text('FILE_ATTACHMENT_MARKER')
   (work/'bad-selection.json').write_text('{invalid')
   (config/'config.toml').write_text(f'backend="openai"\nmodel="gpt-4.1"\nbase_url="http://127.0.0.1:{server.server_port}/v1"\nwire_api="responses"\nrequires_openai_auth=false\nmax_retries=0\n')
   with (config/'config.toml').open('a') as f:
    for name,levels in [('ux-reasoner',['none','low','high']),('ux-fast',[])]:
     f.write('\n[[model_overrides]]\nprovider="openai"\nid='+json.dumps(name)+'\nversion="ux123-fixture-v1"\ncontext_window=64000\nmax_output_tokens=8192\ntext=true\nimages=true\nfiles=true\ntools=true\nstreaming=true\nreasoning_levels='+json.dumps(levels)+'\n')
   (config/'auth.json').write_text(json.dumps({'OPENAI_API_KEY':'ux123-fixture'}));(config/'auth.json').chmod(0o600)
   env={k:v for k,v in os.environ.items() if not k.startswith(('ZENPI_','OPENAI_'))};env.update(ZENPI_HOME=str(config),TERM='xterm-256color')
   terminal=Terminal(binary,root,env);terminals.append(terminal)
   checkpoint=config/'project-tabs.json';journal=root/'sessions/initial.jsonl'
   def saved():return json.loads(checkpoint.read_text())
   def draft():
    v=saved();return next(x['draft']['input'] for x in v['project_state'] if x['name']==v['projects'][v['active']])
   def screen():return screen_text(bytes(terminal.output)).decode()
   def ready(number):
    terminal.expect(f'REPLY_{number:02}'.encode());terminal.wait(lambda:f'REPLY_{number:02}' in journal.read_text())
    terminal.wait(lambda:len(screen().splitlines())>1 and any(label in screen().splitlines()[1] for label in (' Ready |', ' Resources ready |')))
   terminal.type_text('/skill:s');terminal.expect(b'Sample skill metadata');terminal.expect(b'SKILL.md');assert 'ONLY_SELECTED_SKILL' not in screen()
   source_hash=hashlib.sha256(skill.read_bytes()).hexdigest();terminal.expect(source_hash[:12].encode())
   result['source_path']=str(skill.resolve());result['source_sha256']=source_hash;result['menu_screens']={}
   assert len(str(skill.resolve()))>512
   terminal.wait(lambda:draft()=='/skill:s')
   for columns in (80,42,16,1,140):
    fcntl.ioctl(terminal.fd,termios.TIOCSWINSZ,struct.pack('HHHH',40,columns,0,0))
    if columns>=16:
     terminal.wait(lambda:'SKILL.md' in screen_text(bytes(terminal.output),width=columns).decode())
     shot=screen_text(bytes(terminal.output),width=columns).decode();assert 'ONLY_SELECTED_SKILL' not in shot
     if columns>=42:assert source_hash[:12] in shot
     result['menu_screens'][str(columns)]=shot
    else:drain(terminal.fd,terminal.output,.1)
    assert draft()=='/skill:s'
   terminal.expect(b'Sample skill metadata');terminal.expect(b'SKILL.md')
   result['checks']['long_unicode_space_source_visible_across_narrow_resize']=True
   terminal.write(b'\t');terminal.wait(lambda:draft()=='/skill:sample ')
   terminal.write(b'literal $(no-run)');terminal.deliberate_enter();ready(1);assert 'ONLY_SELECTED_SKILL_ONE' in json.dumps(requests[-1]);assert 'literal $(no-run)' in json.dumps(requests[-1]);assert str(skill.resolve()) in requests[-1]['instructions'];assert source_hash in requests[-1]['instructions'];assert 'ONLY_SELECTED_SKILL' not in journal.read_text();result['checks']['actual_skill_owner_and_provenance']=True
   terminal.command('/skills');terminal.expect(b'Resources ready');terminal.wait(lambda:any(str(skill.resolve()) in message['text'] for row in saved()['project_state'] for message in row['messages']));assert len(requests)==1;result['resource_query_messages']=[message['text'] for row in saved()['project_state'] for message in row['messages'] if str(skill.resolve()) in message['text']];result['checks']['full_source_query_retained_after_menu_abbreviation']=True
   result['source_scroll_screens']=[]
   for _ in range(28):
    drain(terminal.fd,terminal.output,.03);result['source_scroll_screens'].append(screen())
    terminal.write(b'\x1b[5~')
   visible_sources=''.join(c for c in ''.join(result['source_scroll_screens']) if not c.isspace() and c not in '│─┌┐└┘●')
   assert all(marker in visible_sources for marker in ('SKILL.md','Resourcesources(fullpaths):','工作','来源space'))
   terminal.write(b'\x1b[F');result['checks']['full_source_index_reachable_by_real_scroll']=True
   for index in range(20):
    extra=work/f'.zenpi/skills/zz-extra-{index:02}';extra.mkdir()
    (extra/'SKILL.md').write_text(f'---\nname: zz-extra-{index:02}\ndescription: Extra skill source\ndisable-model-invocation: true\n---\nEXTRA_BODY_NOT_IN_INSPECTOR\n')
    (work/f'.zenpi/prompts/zz-extra-{index:02}.md').write_text('---\ndescription: Extra template source\n---\nEXTRA_TEMPLATE_NOT_IN_INSPECTOR\n')
   terminal.command('/reload');terminal.expect(b'Resources ready');terminal.wait(lambda:draft()=='')
   terminal.command('/templates');terminal.expect(b'Resources ready');terminal.wait(lambda:draft()=='')
   summaries=[]
   for row in saved()['project_state']:
    for message in row['messages']:
     if message['text'].startswith('Resource sources (full paths):'):
      summaries.append(json.loads(message['text'].split('\n\n',1)[1])['resources'])
   assert summaries and any(value.get('skills_total',0)>16 and len(value['skills'])==16 and value.get('templates_total',0)>16 and len(value['templates'])==16 for value in summaries)
   result['checks']['owner_summary_limits_remain_sixteen_with_more_resources']=True
   result['inspected_sources']=[]
   terminal.write(b'\x1b[200~inspector-kill\x1b[201~');terminal.wait(lambda:draft()=='inspector-kill');terminal.write(b'\x15');terminal.wait(lambda:draft()=='')
   for command,path in [('/skill:zz-extra-19',work/'.zenpi/skills/zz-extra-19/SKILL.md'),('/zz-extra-19',work/'.zenpi/prompts/zz-extra-19.md')]:
    terminal.type_text(command);terminal.wait(lambda:draft()==command);terminal.expect(b'F2 source');before=len(requests);offset=len(terminal.output)
    terminal.write(b'\x1bOQ');terminal.expect(b'Inspect / copy transcript blocks');terminal.expect(b'Resource source');terminal.expect(b'SHA256:')
    terminal.write(b'\x19');drain(terminal.fd,terminal.output,.1);assert draft()==command and len(requests)==before and b'\x1b]52;' not in bytes(terminal.output[offset:])
    result['checks']['source_inspector_ctrl_y_does_not_yank_copy_or_submit']=True
    assert 'EXTRA_BODY_NOT_IN_INSPECTOR' not in screen() and 'EXTRA_TEMPLATE_NOT_IN_INSPECTOR' not in screen()
    for columns in (42,1,140):
     fcntl.ioctl(terminal.fd,termios.TIOCSWINSZ,struct.pack('HHHH',40,columns,0,0));drain(terminal.fd,terminal.output,.08)
     assert draft()==command
    terminal.expect(b'Resource source');terminal.write(b'\r');terminal.wait(lambda:b'\x1b]52;c;' in bytes(terminal.output[offset:]))
    payload=bytes(terminal.output[offset:]).split(b'\x1b]52;c;',1)[1].split(b'\x07',1)[0];copied=base64.b64decode(payload).decode()
    assert json.dumps(str(path.resolve()),ensure_ascii=False) in copied and hashlib.sha256(path.read_bytes()).hexdigest() in copied
    terminal.wait(lambda:'Inspect / copy transcript blocks' not in screen());assert draft()==command and len(requests)==before
    terminal.write(b'\x1bOQ');terminal.expect(b'Resource source');terminal.write(b'\x1b');terminal.wait(lambda:'Inspect / copy transcript blocks' not in screen());assert draft()==command and len(requests)==before
    result['inspected_sources'].append({'command':command,'path':str(path.resolve()),'copied':copied});terminal.write(b'\x15');terminal.wait(lambda:draft()=='')
   result['checks']['seventeenth_plus_skill_and_template_full_source_inspector_exact_copy_no_submit']=True
   terminal.write(b'/fixture-h\t');terminal.wait(lambda:draft()=='/fixture-hi ');terminal.write(b'Ada');terminal.deliberate_enter();ready(2);assert 'TEMPLATE_EXPANDED: Ada | Ada' in json.dumps(requests[-1]);assert 'TEMPLATE_EXPANDED' not in journal.read_text();result['checks']['actual_template_expansion']=True
   terminal.type_text('inspect @工作');terminal.expect('工作 folder/'.encode());terminal.write(b'\t');terminal.expect(b'note file.txt');terminal.write(b'\t');terminal.wait(lambda:'note file.txt" ' in draft());terminal.write(b'please');terminal.deliberate_enter();ready(3)
   wire=json.dumps(requests[-1]);assert base64.b64encode(b'FILE_ATTACHMENT_MARKER').decode() in wire;assert 'FILE_ATTACHMENT_MARKER' not in journal.read_text();assert 'sha256' in journal.read_text();result['checks']['actual_file_attachment_with_unicode_space_path']=True
   terminal.command('/missing-command');terminal.wait(lambda:draft()=='/missing-command');assert len(requests)==3;terminal.write(b'\x15');result['checks']['unknown_command_preserves_draft']=True
   terminal.command('/attach ../denied');terminal.wait(lambda:draft()=='/attach ../denied');assert len(requests)==3;terminal.write(b'\x15');result['checks']['denied_path_preserves_draft']=True
   terminal.command('slow-resource-case');terminal.wait(slow.is_set);terminal.expect(b'BUSY_RESOURCE_MARKER')
   terminal.type_text('/skill:zz-extra-19');terminal.wait(lambda:draft()=='/skill:zz-extra-19');terminal.write(b'\x1bOQ');terminal.expect(b'Resource owner busy');assert draft()=='/skill:zz-extra-19' and len(requests)==4;terminal.cancel_picker();terminal.write(b'\x15');result['checks']['busy_source_owner_failure_visible_and_draft_preserved']=True
   terminal.type_text('/model');terminal.expect(b'idle only');terminal.cancel_picker();terminal.write(b'\x15')
   terminal.command('/reasoning high');terminal.wait(lambda:draft()=='/reasoning high');terminal.expect(b'reasoning selection requires an idle project');terminal.write(b'\x15');result['checks']['busy_reasoning_change_rejected']=True
   set_skill('TWO');terminal.command('/reload');terminal.expect(b'Input queued');release.set();ready(4);terminal.expect(b'Resources ready');result['checks']['busy_reload_queued_without_steering']=True
   terminal.command('/skill:sample after-reload');ready(5);assert 'ONLY_SELECTED_SKILL_TWO' in json.dumps(requests[-1]);result['checks']['reload_publishes_new_metadata_and_body']=True
   terminal.command('/reload bad-selection.json');terminal.wait(lambda:draft()=='/reload bad-selection.json');assert len(requests)==5;terminal.write(b'\x15');result['checks']['failed_reload_preserves_draft_and_catalogue']=True
   terminal.command('/skill:sample after-failure');ready(6);assert 'ONLY_SELECTED_SKILL_TWO' in json.dumps(requests[-1])
   terminal.type_text('/skill:');terminal.expect(b'/skill:sample');terminal.cancel_picker();terminal.wait(lambda:draft()=='/skill:');result['checks']['escape_preserves_input']=True
   terminal.close(preserve_draft=True);terminal=Terminal(binary,root,env);terminals.append(terminal);terminal.wait(lambda:draft()=='/skill:');terminal.write(b'\x15');terminal.command('/skill:sample after-restart');ready(7);assert 'ONLY_SELECTED_SKILL_TWO' in json.dumps(requests[-1]);result['checks']['independent_process_restart']=True
   terminal.type_text('/model ux-r');terminal.expect(b'ux-reasoner');terminal.expect(b'context 64000');terminal.write(b'\t');terminal.deliberate_enter();terminal.expect(b'model selected: ux-reasoner')
   terminal.type_text('/reasoning ');terminal.expect(b'/reasoning high');terminal.expect(b'default omits');assert '/reasoning ultra' not in screen();terminal.cancel_picker();terminal.write(b'\x15');terminal.type_text('/reasoning high');terminal.write(b'\t');terminal.deliberate_enter();terminal.expect(b'reasoning selected: high')
   terminal.command('model-effort-high');ready(8);assert requests[-1]['model']=='ux-reasoner';assert requests[-1]['reasoning']['effort']=='high';result['checks']['actual_model_and_effort_request']=True
   terminal.command('/reasoning ultra');terminal.wait(lambda:draft()=='/reasoning ultra');assert len(requests)==8;terminal.write(b'\x15')
   terminal.type_text('/model ux-fast');terminal.write(b'\t');terminal.deliberate_enter();terminal.wait(lambda:draft().strip()=='/model ux-fast');assert len(requests)==8;terminal.write(b'\x15');result['checks']['invalid_effort_and_incompatible_model_preserve_input']=True
   terminal.type_text('/reasoning low');terminal.write(b'\t');terminal.deliberate_enter();terminal.expect(b'reasoning selected: low');terminal.close();terminal=Terminal(binary,root,env);terminals.append(terminal)
   terminal.command('restored-model-effort-low');ready(9);assert requests[-1]['model']=='ux-reasoner';assert requests[-1]['reasoning']['effort']=='low';result['checks']['independent_model_effort_restart']=True
   terminal.type_text('/reasoning none');terminal.write(b'\t');terminal.deliberate_enter();terminal.expect(b'reasoning selected: none');terminal.command('explicit-none');ready(10);assert requests[-1]['reasoning']['effort']=='none';result['checks']['explicit_none_is_a_real_level']=True
   terminal.type_text('/reasoning default');terminal.write(b'\t');terminal.deliberate_enter();terminal.expect(b'reasoning selected: provider default');terminal.type_text('/model ux-fast');terminal.write(b'\t');terminal.deliberate_enter();terminal.expect(b'model selected: ux-fast');terminal.command('default-fast');ready(11);assert requests[-1]['model']=='ux-fast';assert 'reasoning' not in requests[-1]
   terminal.close();terminal=Terminal(binary,root,env);terminals.append(terminal);terminal.command('restored-default-fast');ready(12);assert requests[-1]['model']=='ux-fast';assert 'reasoning' not in requests[-1];result['checks']['default_omission_and_model_restart']=True
   terminal.close();result['checks']['terminal_restored']=True;result['request_count']=len(requests);assert len(requests)==12;result['passed']=True
 finally:
  release.set();server.shutdown();args.evidence.parent.mkdir(parents=True,exist_ok=True)
  args.evidence.with_suffix('.pty.log').write_bytes(b'\nNEXT PROCESS\n'.join(bytes(t.output) for t in terminals))
  args.evidence.with_suffix('.http.json').write_text(json.dumps(requests,ensure_ascii=False,indent=2)+'\n')
  args.evidence.write_text(json.dumps(result,indent=2)+'\n')
  for t in terminals:t.cleanup()
 print(json.dumps(result,indent=2))
if __name__=='__main__':main()
