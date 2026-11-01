#!/usr/bin/env python3
"""Offline release workflow tests: no signing keys, Keychain writes, or Apple uploads."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

SCRIPTS = Path(__file__).resolve().parent
IDENTITY = 'A' * 40
MOCK = r'''#!/usr/bin/env python3
import json, os, pathlib, shutil, sys
name = pathlib.Path(sys.argv[0]).name
args = sys.argv[1:]
mode = os.environ.get('RELEASE_TEST_MODE', '')
with open(os.environ['RELEASE_TEST_LOG'], 'a') as f:
    f.write(json.dumps([name, *args]) + '\n')
if name == 'uname': print('Darwin')
elif name == 'security':
    print('  1) ' + 'A' * 40 + ' "Developer ID Application: Test (TEAM)"')
    if mode == 'ambiguous': print('  2) ' + 'B' * 40 + ' "Developer ID Application: Test (TEAM)"')
elif name == 'lipo':
    if '-archs' in args:
        value = pathlib.Path(args[-1]).read_text()
        print(value if value in ['arm64', 'x86_64', 'arm64 x86_64'] else 'arm64')
    else: pathlib.Path(args[args.index('-output') + 1]).write_text('arm64 x86_64')
elif name == 'otool':
    if mode == 'otool-failure': sys.exit(1)
    print('SystemMonitor:')
    print('\t' + ('/opt/homebrew/lib/private.dylib' if mode == 'external-library' else '/usr/lib/libSystem.B.dylib') + ' (compatibility version 1.0.0)')
elif name == 'rustup': print(os.environ.get('RELEASE_TEST_TARGETS', 'aarch64-apple-darwin'))
elif name == 'cargo' and '--target' in args:
    target = args[args.index('--target') + 1]
    path = pathlib.Path(os.environ['CARGO_TARGET_DIR']) / target / 'release/SystemMonitor'
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text('x86_64' if target.startswith('x86_64') else 'arm64')
elif name == 'codesign' and '--verify' in args and args[-1].endswith('.dmg') and mode == 'bad-dmg-signature':
    sys.exit(1)
elif name == 'codesign' and '-d' in args:
    print('Authority=Developer ID Application: Test (TEAM)\nCodeDirectory v=20500 flags=0x10000(runtime) hashes=1\nTimestamp=now', file=sys.stderr)
    if mode == 'verbose-signature': print('extra signature detail\n' * 10000, file=sys.stderr)
elif name == 'ditto':
    if '-c' in args: pathlib.Path(args[-1]).write_bytes(b'zip')
    else: shutil.copytree(args[-2], args[-1], symlinks=True)
elif name == 'hdiutil':
    if args[0] == 'create': pathlib.Path(args[-1]).write_bytes(b'dmg')
    elif args[0] == 'attach':
        mount = pathlib.Path(args[args.index('-mountpoint') + 1])
        shutil.copytree(pathlib.Path(args[-1]).parent / 'SystemMonitor.app', mount / 'SystemMonitor.app')
        (mount / 'Applications').symlink_to('/Applications')
elif name == 'xcrun':
    if args[0] == '--find': print('/mock/' + args[1])
    elif args[:2] == ['notarytool', 'submit']:
        rejected = mode == 'reject-app' or (mode == 'reject-dmg' and args[2].endswith('.dmg'))
        status = 'Invalid' if rejected else ('In Progress' if mode == 'timeout' else 'Accepted')
        print(json.dumps({'id': '00000000-0000-0000-0000-000000000001', 'status': status}))
        if mode in ['timeout', 'accepted-exit-failure']: sys.exit(69)
    elif args[:2] == ['notarytool', 'log']: pathlib.Path(args[-1]).write_text('{}')
'''
BUNDLE = r'''#!/usr/bin/env python3
import os, pathlib, plistlib
app = pathlib.Path(os.environ['BUNDLE_OUTPUT'])
(app / 'Contents/MacOS').mkdir(parents=True)
(app / 'Contents/MacOS/SystemMonitor').write_text('binary')
(app / 'Contents/Info.plist').write_bytes(plistlib.dumps({'CFBundleShortVersionString': '0.1.0'}))
'''


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='System Monitor release test ')
        self.root = Path(self.temp.name)
        (self.root / 'scripts').mkdir()
        for name in ['release-dmg.sh', 'create-dmg.sh', 'verify-dmg.sh']:
            shutil.copy2(SCRIPTS / name, self.root / 'scripts' / name)
        for name, body in [('bundle.sh', BUNDLE), ('validate.sh', '#!/bin/sh\nexit 0\n')]:
            path = self.root / 'scripts' / name
            path.write_text(body)
            path.chmod(0o755)
        (self.root / 'bin').mkdir()
        for name in ['uname', 'security', 'lipo', 'otool', 'rustup', 'codesign', 'ditto', 'hdiutil', 'xcrun', 'spctl', 'cargo', 'rustc']:
            path = self.root / 'bin' / name
            path.write_text(MOCK)
            path.chmod(0o755)
        self.env = {**os.environ, 'PATH': str(self.root / 'bin') + ':' + os.environ['PATH'],
                    'DEVELOPER_ID': IDENTITY, 'NOTARY_PROFILE': 'test-profile',
                    'DIST_DIR': str(self.root / 'output with spaces'),
                    'CARGO_TARGET_DIR': str(self.root / 'cargo target with spaces'),
                    'RELEASE_TEST_LOG': str(self.root / 'events.jsonl')}
        self.env.pop('RELEASE_TEST_MODE', None)

    def tearDown(self):
        self.temp.cleanup()

    def run_release(self, *args, mode=''):
        env = {**self.env, 'RELEASE_TEST_MODE': mode}
        return subprocess.run(['/bin/bash', str(self.root / 'scripts/release-dmg.sh'), *args], env=env, text=True, capture_output=True)

    def events(self):
        path = self.root / 'events.jsonl'
        return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []

    def images(self):
        return list(Path(self.env['DIST_DIR']).glob('*.dmg'))

    def test_full_release_orders_notarization_and_keeps_receipts(self):
        result = self.run_release()
        self.assertEqual(result.returncode, 0, result.stderr)
        jobs = [e for e in self.events() if e[:3] == ['xcrun', 'notarytool', 'submit']]
        self.assertEqual(len(jobs), 2)
        self.assertTrue(jobs[0][3].endswith('.zip'))
        self.assertTrue(jobs[1][3].endswith('.dmg'))
        self.assertEqual(len(self.images()), 1)
        self.assertNotIn('signed-only', self.images()[0].name)
        self.assertTrue(Path(str(self.images()[0]) + '.sha256').exists())
        self.assertEqual(len(list(Path(self.env['DIST_DIR']).glob('*.logs/*submission.json'))), 2)
        self.assertEqual(len(list(Path(self.env['DIST_DIR']).glob('.SystemMonitor-release.*'))), 0)
        again = self.run_release()
        self.assertNotEqual(again.returncode, 0)
        self.assertIn('Release already exists', again.stderr)
        self.assertEqual(len([e for e in self.events() if e[:3] == ['xcrun', 'notarytool', 'submit']]), 2)

    def test_signed_only_never_uploads(self):
        self.env.pop('NOTARY_PROFILE')
        result = self.run_release('--signed-only')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('signed-only', self.images()[0].name)
        self.assertFalse(any(e[0] == 'xcrun' for e in self.events()))

    def test_verbose_signature_does_not_fail_from_closed_pipe(self):
        result = self.run_release('--signed-only', mode='verbose-signature')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(self.images()), 1)

    def test_apple_rejections_or_timeout_do_not_publish(self):
        for mode in ['reject-app', 'reject-dmg', 'timeout', 'accepted-exit-failure']:
            with self.subTest(mode=mode):
                result = self.run_release(mode=mode)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('did not finish as Accepted', result.stderr)
                self.assertEqual(self.images(), [])

    def test_architecture_assembly_and_final_signature_failure(self):
        result = self.run_release(mode='bad-dmg-signature')
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.images(), [])
        self.env['RELEASE_TEST_TARGETS'] = 'aarch64-apple-darwin\nx86_64-apple-darwin'
        result = self.run_release('--arch', 'universal', '--signed-only')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('universal-signed-only', self.images()[0].name)
        self.assertTrue(any(e[:2] == ['lipo', '-create'] for e in self.events()))

    def test_bad_identity_and_missing_credentials_fail_early(self):
        self.env['DEVELOPER_ID'] = 'Developer ID Application: Test (TEAM)'
        result = self.run_release(mode='ambiguous')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('exactly one', result.stderr)
        self.env.pop('NOTARY_PROFILE')
        result = self.run_release()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('NOTARY_PROFILE', result.stderr)
        self.assertFalse(any(e[:3] == ['xcrun', 'notarytool', 'submit'] for e in self.events()))

    def test_missing_cross_target_and_external_library_fail_closed(self):
        result = self.run_release('--arch', 'universal')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('rustup target add x86_64-apple-darwin', result.stderr)
        for mode in ['external-library', 'otool-failure']:
            result = self.run_release(mode=mode)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(self.images(), [])


if __name__ == '__main__':
    unittest.main()
