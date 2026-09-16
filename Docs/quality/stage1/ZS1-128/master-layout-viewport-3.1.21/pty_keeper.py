"""Keep the controlling terminal alive until its parent observes restored flags.

Runs exactly one zenpi child, reaps it, reports the result over a private pipe,
then waits for an observation acknowledgement. No product retry or input driver.
"""
import json
import os
import signal
import subprocess
import sys

report_fd, ack_fd = map(int, sys.argv[1:3])
child = subprocess.Popen(sys.argv[3:])

def terminate_child(_signal, _frame):
    if child.poll() is None:
        child.terminate()

signal.signal(signal.SIGTERM, terminate_child)
code = child.wait()
os.write(report_fd, json.dumps(dict(pid=child.pid, exit_code=code, reaped=True)).encode() + b'\n')
os.close(report_fd)
os.read(ack_fd, 1)
os.close(ack_fd)
