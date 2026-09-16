import os,pathlib,time
root=pathlib.Path.cwd()
with (root/'effects.txt').open('a') as f:
 f.write('START\n');f.flush();os.fsync(f.fileno())
(root/'child-pid.txt').write_text(str(os.getpid()))
os.write(1,b'CHILD_PARTIAL_319\n'+b'x'*4096)
os.write(2,b'CHILD_STDERR_319\n')
end=time.monotonic()+15
while not (root/'release-child').exists() and time.monotonic()<end: time.sleep(.01)
(root/'child-done').write_text('finished own bounded fixture\n')
