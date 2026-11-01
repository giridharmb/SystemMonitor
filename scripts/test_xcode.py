#!/usr/bin/env python3
"""Offline checks for Xcode's Cargo bridge; no compiler or signing keys needed."""
import os
from pathlib import Path
import plistlib
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().with_name('xcode-build.sh')
MOCK = '''#!/usr/bin/env python3
import json, os, pathlib, sys
name = pathlib.Path(sys.argv[0]).name
if name == 'rustc': print('host: aarch64-apple-darwin')
elif name == 'rustup': print('aarch64-apple-darwin')
elif 'metadata' in sys.argv:
    print(json.dumps({'packages': [{'name': 'SystemMonitor', 'version': '1.2.3'}]}))
else:
    profile = 'release' if '--release' in sys.argv else 'debug'
    binary = pathlib.Path(os.environ['CARGO_TARGET_DIR']) / profile / 'SystemMonitor'
    binary.parent.mkdir(parents=True, exist_ok=True)
    binary.write_text(profile)
'''


class XcodeBuildTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='xcode bridge ')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        tools = self.root / 'cargo-home' / 'bin'
        tools.mkdir(parents=True)
        for name in ('cargo', 'rustc', 'rustup'):
            tool = tools / name
            tool.write_text(MOCK)
            tool.chmod(0o755)
        self.env = dict(os.environ, SRCROOT=str(self.root),
                        CARGO_HOME=str(tools.parent),
                        CARGO_TARGET_DIR=str(self.root / 'target'),
                        TARGET_BUILD_DIR=str(self.root / 'products'),
                        EXECUTABLE_PATH='SystemMonitor.app/Contents/MacOS/SystemMonitor',
                        DERIVED_FILE_DIR=str(self.root / 'derived'),
                        PRODUCT_BUNDLE_IDENTIFIER='test.systemmonitor',
                        MACOSX_DEPLOYMENT_TARGET='13.0', ARCHS='arm64')

    def run_build(self, **settings):
        return subprocess.run(['/bin/bash', str(SCRIPT)],
                              env=dict(self.env, **settings), capture_output=True, text=True)

    def test_debug_release_bundle_and_atomic_replacement(self):
        binary = Path(self.env['TARGET_BUILD_DIR']) / self.env['EXECUTABLE_PATH']
        for configuration, profile in [('Debug', 'debug'), ('Release', 'release')]:
            binary.parent.mkdir(parents=True, exist_ok=True)
            binary.write_text('previous executable')
            with binary.open() as previous:
                result = self.run_build(CONFIGURATION=configuration)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(previous.read(), 'previous executable')
            self.assertEqual(binary.read_text(), profile)
            self.assertTrue(os.access(binary, os.X_OK))
            with (self.root / 'derived/SystemMonitor-Info.plist').open('rb') as f:
                info = plistlib.load(f)
            self.assertEqual(info['CFBundleShortVersionString'], '1.2.3')
            self.assertEqual(info['CFBundleIdentifier'], 'test.systemmonitor')
            self.assertEqual(info['CFBundleIconFile'], 'AppIcon')
            self.assertEqual(info['LSMinimumSystemVersion'], '13.0')

    def test_unsupported_configuration_and_architecture(self):
        for settings, expected in [({'CONFIGURATION': 'Custom'}, 'Unsupported Xcode configuration'),
                                   ({'ARCHS': 'mips'}, 'Unsupported Mac architecture'),
                                   ({'ARCHS': 'x86_64'}, 'rustup target add x86_64-apple-darwin')]:
            with self.subTest(settings=settings):
                result = self.run_build(**settings)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(expected, result.stderr)


if __name__ == '__main__':
    unittest.main()
