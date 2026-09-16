"""Exercise cold-start evidence retention without running/building zenpi."""
import base64
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

import bench_runtime as bench


class StartupEvidence(unittest.TestCase):
    def fixture(self, root, body):
        binary = root / 'target/release/zenpi'
        binary.parent.mkdir(parents=True)
        binary.write_text('#!' + sys.executable + '\n' + body)
        binary.chmod(0o755)
        return binary

    def test_success_keeps_exact_streams_and_timing_identity(self):
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            binary = self.fixture(root, 'import sys\nsys.stdin.read()\nsys.stdout.buffer.write(b\'{"success":true}\\r\\n\')\n')
            with patch.object(bench, 'ROOT', root), patch.object(bench, 'run') as build:
                _, receipt = bench.build_and_measure_startup(3, root/'evidence')
            build.assert_called_once()
            self.assertEqual(receipt['elapsed_ms']['sample_count'], 3)
            self.assertEqual(receipt['binary_sha256'], hashlib.sha256(binary.read_bytes()).hexdigest())
            self.assertEqual(len({r['wrapper_pid'] for r in receipt['observations']}), 3)
            for index, r in enumerate(receipt['observations']):
                self.assertEqual(json.loads((root/f'evidence/sample-{index:02}.json').read_text()), r)
                self.assertEqual(base64.b64decode(r['stdout']['prefix_base64']), b'{"success":true}\r\n')
                self.assertFalse(r['stdout']['truncated'])
                self.assertEqual(r['returncode'], 0)
                self.assertFalse(r['timed_out'])
                self.assertGreater(r['peak_rss_bytes'], 0)
                self.assertLessEqual(r['process_observed_ms'], r['elapsed_ms'])
                self.assertIn('not the zenpi child PID', r['pid_scope'])

    def test_failed_sample_is_durable_before_exception(self):
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            self.fixture(root, 'import sys\nsys.stdout.write("partial output")\nsys.stderr.write("fixture failed")\nsys.exit(7)\n')
            with patch.object(bench, 'ROOT', root), patch.object(bench, 'run'):
                with self.assertRaisesRegex(RuntimeError, 'rc=7'):
                    bench.build_and_measure_startup(3, root/'evidence')
            receipt = json.loads((root/'evidence/sample-00.json').read_text())
            self.assertEqual(receipt['returncode'], 7)
            self.assertEqual(base64.b64decode(receipt['stdout']['prefix_base64']), b'partial output')
            self.assertIn(b'fixture failed', base64.b64decode(receipt['stderr']['prefix_base64']))
            self.assertFalse((root/'evidence/sample-01.json').exists())

    def test_timeout_retains_partial_output_and_kills_probe_group(self):
        with tempfile.TemporaryDirectory() as raw:
            marker = Path(raw)/'late-write'
            child_code = 'import time,pathlib;time.sleep(.4);pathlib.Path('+repr(str(marker))+').touch()'
            code = 'import subprocess,sys,time;subprocess.Popen([sys.executable,"-c",'+repr(child_code)+']);print("started",flush=True);time.sleep(9)'
            observation = {}
            with self.assertRaises(subprocess.TimeoutExpired):
                bench.timed_process([sys.executable,'-c',code],env=os.environ.copy(),payload='',
                                    observation=observation,timeout=.15)
            bench.write_startup_observation(Path(raw)/'evidence',0,observation)
            self.assertTrue(observation['timed_out'])
            self.assertIn(b'started',base64.b64decode(observation['stdout']['prefix_base64']))
            time.sleep(.5)
            self.assertFalse(marker.exists())

    def test_export_is_bounded_and_cannot_overwrite_first_sample(self):
        raw = b'X'*70000
        with tempfile.TemporaryDirectory() as temp:
            observation = {'_streams':{'stdout':raw}}
            bench.write_startup_observation(Path(temp),0,observation)
            record = observation['stdout']
            self.assertEqual(record['bytes'],70000)
            self.assertEqual(record['sha256'],hashlib.sha256(raw).hexdigest())
            self.assertEqual(len(base64.b64decode(record['prefix_base64'])),65536)
            self.assertTrue(record['truncated'])
            before = (Path(temp)/'sample-00.json').read_bytes()
            with self.assertRaises(FileExistsError):
                bench.write_startup_observation(Path(temp),0,{'replacement':True})
            self.assertEqual((Path(temp)/'sample-00.json').read_bytes(),before)


if __name__ == '__main__':
    unittest.main()
