"""Reports and logs never go next to the converted files: the installed app keeps
them in a logs folder for 30 days; the portable Windows app writes nothing."""
import os, subprocess, tempfile, time, unittest
from pathlib import Path
from test_guards import fixture, BIN
from test_slide_size import deck

WINDOWS_ENGINE = 'windows-target' in str(BIN)

def run(*args, **env):
    clean = {k: v for k, v in os.environ.items() if k not in ('DECKWRIGHT_REPORTS_NEXT_TO_OUTPUT', 'DECKWRIGHT_LOG_DIR')}
    return subprocess.run([str(BIN), *map(str, args)], capture_output=True, text=True, env={**clean, **env})

class Reports(unittest.TestCase):
    def test_installed_writes_reports_only_to_the_logs_folder(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d); work, logs = d / 'work', d / 'logs'
            work.mkdir(); logs.mkdir()
            old, recent = logs / 'old.report.json', logs / 'recent.report.json'
            old.write_text('{}'); recent.write_text('{}')
            month = time.time() - 31 * 86400
            os.utime(old, (month, month))
            a, b = work / 'a.pptx', work / 'b.pptx'
            fixture(a); deck(b, 9144000, 6858000)
            self.assertEqual(run('--merge', work / 'merged.pptx', a, b, DECKWRIGHT_LOG_DIR=str(logs)).returncode, 0)
            self.assertEqual(run('--resize', work / 'wide.pptx', b, DECKWRIGHT_LOG_DIR=str(logs)).returncode, 0)
            bad = work / 'bad.pptx'; bad.write_bytes(b'not a zip')
            self.assertEqual(run('--extract-notes', work / 'notes.pptx', bad, DECKWRIGHT_LOG_DIR=str(logs)).returncode, 1)
            self.assertEqual(sorted(p.name for p in work.iterdir()), ['a.pptx', 'b.pptx', 'bad.pptx', 'merged.pptx', 'wide.pptx'])
            names = sorted(p.name for p in logs.iterdir())
            self.assertNotIn('old.report.json', names)  # older than 30 days
            self.assertIn('recent.report.json', names)
            self.assertTrue(any(n.startswith('merged-') and n.endswith('.report.json') for n in names), names)
            self.assertTrue(any(n.startswith('merged-') and n.endswith('.report.txt') for n in names), names)
            self.assertTrue(any(n.startswith('wide-') and n.endswith('.report.json') for n in names), names)
            self.assertTrue(any(n.startswith('Deckwright-ERROR-') for n in names), names)

    @unittest.skipUnless(WINDOWS_ENGINE, 'portable copies exist only on Windows')
    def test_portable_writes_nothing_but_the_output(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d); work, home = d / 'work', d / 'home'
            work.mkdir(); home.mkdir()
            a, b = work / 'a.pptx', work / 'b.pptx'
            fixture(a); deck(b, 9144000, 6858000)
            env = {'HOME': str(home), 'LOCALAPPDATA': str(home), 'APPDATA': str(home)}
            r = run('--merge', work / 'merged.pptx', a, b, **env)
            self.assertEqual(r.returncode, 0, r.stderr); self.assertIn('Report: none', r.stdout)
            bad = work / 'bad.pptx'; bad.write_bytes(b'not a zip')
            self.assertEqual(run('--resize', work / 'x.pptx', bad, **env).returncode, 1)
            self.assertEqual(sorted(p.name for p in work.iterdir()), ['a.pptx', 'b.pptx', 'bad.pptx', 'merged.pptx'])
            self.assertEqual(list(home.rglob('*')), [])

if __name__ == '__main__':
    unittest.main()
