"""License gate boundaries. No network or repository mutation."""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location('package_licenses', Path(__file__).parents[1] / 'package-licenses.py')
GATE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GATE)


class GateTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.revision = 'a' * 40
        self.repository = 'https://github.com/example/vendor.git'
        self.source = 'git+https://github.com/example/vendor.git?rev=' + self.revision + '#' + self.revision
        license_path = self.root / 'licenses/dependencies/texts/vendor.txt'
        license_path.parent.mkdir(parents=True)
        self.raw = b'Publisher original license text\n'
        license_path.write_bytes(self.raw)
        notice = {'path': 'licenses/dependencies/texts/vendor.txt', 'sha256': hashlib.sha256(self.raw).hexdigest(), 'source': 'https://github.com/example/vendor/blob/' + self.revision + '/LICENSE', 'originalPath': 'LICENSE', 'gitBlob': hashlib.sha1(b'blob ' + str(len(self.raw)).encode() + b'\0' + self.raw).hexdigest()}
        self.git = {'name': 'vendor', 'version': '1.0.0', 'source': self.source, 'repository': self.repository, 'revision': self.revision, 'manifestGitBlob': 'b' * 40, 'files': [notice]}
        self.registry = {'name': 'registry', 'version': '2.0.0', 'crateSha256': 'c' * 64, 'files': [dict(notice)]}
        self.catalog = {'packages': [self.registry], 'toolchains': [], 'gitPackages': [self.git]}
        self.lock = [{'name': 'vendor', 'version': '1.0.0', 'source': self.source}, {'name': 'registry', 'version': '2.0.0', 'source': 'registry+https://github.com/rust-lang/crates.io-index', 'checksum': 'c' * 64}]

    def validate(self):
        (self.root / 'licenses/dependencies/catalog.json').write_text(json.dumps(self.catalog))
        lock = '\n'.join('[[package]]\n' + '\n'.join(f'{k} = {json.dumps(v)}' for k, v in entry.items()) for entry in self.lock)
        (self.root / 'Cargo.lock').write_text(lock)
        with patch.object(GATE, 'ROOT', self.root), patch.object(GATE.subprocess, 'check_output', return_value='Cargo.lock\n'):
            return GATE.validate()

    def test_exact_git_and_registry_sources_pass(self):
        self.assertEqual(self.validate(), 1)

    def test_unknown_or_moved_git_revision_fails(self):
        self.lock[0]['source'] = self.source.replace(self.revision, 'd' * 40)
        with self.assertRaises(ValueError): self.validate()

    def test_wrong_git_package_identity_fails(self):
        self.lock[0]['name'] = 'different-package'
        with self.assertRaises(ValueError): self.validate()

    def test_changed_registry_checksum_fails(self):
        self.lock[1]['checksum'] = 'd' * 64
        with self.assertRaises(ValueError): self.validate()

    def test_unknown_registry_version_fails(self):
        self.lock[1]['version'] = '3.0.0'
        with self.assertRaises(ValueError): self.validate()

    def test_tampered_license_bytes_fail(self):
        (self.root / self.git['files'][0]['path']).write_bytes(b'changed')
        with self.assertRaises(ValueError): self.validate()

    def test_notice_git_blob_and_source_provenance_are_checked(self):
        self.git['files'][0]['gitBlob'] = 'd' * 40
        with self.assertRaises(ValueError): self.validate()
        self.git['files'][0]['gitBlob'] = hashlib.sha1(b'blob ' + str(len(self.raw)).encode() + b'\0' + self.raw).hexdigest()
        self.git['files'][0]['source'] = 'https://github.com/other/vendor/blob/' + self.revision + '/LICENSE'
        with self.assertRaises(ValueError): self.validate()

    def test_escape_empty_and_duplicate_git_entries_fail(self):
        self.git['files'][0]['path'] = '../outside.txt'
        with self.assertRaises(ValueError): self.validate()
        self.git['files'] = []
        with self.assertRaises(ValueError): self.validate()
        self.git['files'] = [self.registry['files'][0]]
        self.catalog['gitPackages'].append(self.git)
        with self.assertRaises(ValueError): self.validate()


if __name__ == '__main__':
    unittest.main()
