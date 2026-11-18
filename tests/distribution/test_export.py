"""Synthetic file fixtures only; no installed browser/profile is opened."""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[2] / 'tools/export_distribution.py'


class ExportTests(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location('distribution', SCRIPT)
        self.module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.module)
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        base = Path(self.temp.name)
        self.source, self.package, self.output = (base / name for name in ('source', 'package', 'output'))
        self.source.mkdir()
        (self.package / 'app').mkdir(parents=True)
        (self.source / 'main.rs').write_text('fn main() {}', encoding='utf-8')
        (self.source / 'Start-Browser.cmd').write_text('@echo off\nstart "" "%~dp0app\\RustBrowser.exe"\n', encoding='utf-8')
        (self.package / 'app/RustBrowser.exe').write_bytes(b'synthetic-executable')
        self.source_manifest(['main.rs', 'Start-Browser.cmd', 'BROWSER-OVERLAY.json'])
        self.app_manifest({'app/RustBrowser.exe': hashlib.sha256(b'synthetic-executable').hexdigest()})

    def source_manifest(self, files):
        (self.source / 'BROWSER-OVERLAY.json').write_text(json.dumps({'files': files}), encoding='utf-8')

    def app_manifest(self, files):
        (self.package / 'candidate-manifest.json').write_text(json.dumps({'sha256': files, 'stable_release': False}), encoding='utf-8')

    def run_export(self):
        self.module.export(self.source, self.package, self.output)

    def test_only_selected_code_and_verified_application_are_exported(self):
        (self.source / 'local-report.md').write_text('LOCAL', encoding='utf-8')
        (self.package / 'app/profile.json').write_text('PRIVATE', encoding='utf-8')
        self.run_export()
        self.assertEqual((self.output / 'source/main.rs').read_text(), 'fn main() {}')
        self.assertEqual((self.output / 'app/RustBrowser.exe').read_bytes(), b'synthetic-executable')
        self.assertFalse((self.output / 'source/local-report.md').exists())
        self.assertFalse((self.output / 'app/profile.json').exists())
        self.assertEqual({p.name for p in self.output.iterdir()}, {'app', 'source', 'Start-Browser.cmd', 'README.md', 'SHA256SUMS.txt'})
        for line in (self.output / 'SHA256SUMS.txt').read_text().splitlines():
            digest, name = line.split('  ', 1)
            self.assertEqual(hashlib.sha256((self.output / name).read_bytes()).hexdigest(), digest)

    def test_hash_mismatch_rejected_before_writing(self):
        self.app_manifest({'app/RustBrowser.exe': '0' * 64})
        with self.assertRaises(ValueError):
            self.run_export()
        self.assertFalse(self.output.exists())

    def test_package_change_during_copy_cannot_pass_verification(self):
        copy = self.module.shutil.copy2

        def changed_copy(source, target):
            if Path(source) == self.package / 'app/RustBrowser.exe':
                Path(source).write_bytes(b'changed-after-validation')
            return copy(source, target)

        with patch.object(self.module.shutil, 'copy2', side_effect=changed_copy):
            with self.assertRaises(OSError):
                self.run_export()

    def test_unsafe_or_private_paths_rejected(self):
        for name in ('../outside', 'app/../outside', 'app/Cookies', 'app/profile.json', 'app/passwords.dat', 'app/export.csv', 'app/debug.log', 'app/C:/outside', 'app/CON', 'app/file:stream'):
            with self.subTest(name=name):
                self.app_manifest({name: '0' * 64})
                with self.assertRaises(ValueError):
                    self.run_export()
                self.assertFalse(self.output.exists())

    def test_development_artifacts_rejected_even_when_listed(self):
        for name in ('CONTINUE.md', 'work/report.txt', '.git/config', 'target/debug.exe', 'graphify-out/graph.json'):
            with self.subTest(name=name):
                self.source_manifest([name])
                with self.assertRaises(ValueError):
                    self.run_export()

    def test_missing_source_rejected_before_writing(self):
        self.source_manifest(['missing.rs'])
        with self.assertRaises((ValueError, FileNotFoundError)):
            self.run_export()
        self.assertFalse(self.output.exists())

    def test_existing_output_is_preserved(self):
        self.output.mkdir()
        marker = self.output / 'keep.txt'
        marker.write_text('KEEP', encoding='utf-8')
        with self.assertRaises(FileExistsError):
            self.run_export()
        self.assertEqual(marker.read_text(), 'KEEP')

    def test_output_inside_an_input_is_rejected(self):
        for root in (self.source, self.package):
            with self.subTest(root=root):
                self.output = root / 'delivery'
                with self.assertRaises(ValueError):
                    self.run_export()
                self.assertFalse(self.output.exists())


if __name__ == '__main__':
    unittest.main()
