import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location('package_licenses', Path(__file__).parents[1] / 'package-licenses.py')
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class GitLicenseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.revision = 'a' * 40
        self.repository = 'https://github.com/allowit-hq/example.git'
        self.source = f'git+{self.repository}?rev={self.revision}#{self.revision}'
        directory = self.root / 'licenses/dependencies'
        directory.mkdir(parents=True)
        manifest = b'[package]\nname="vendor"\nversion="0.1.0"\nlicense="MIT"\n'
        license_text = b'MIT License\nExact upstream copyright and grant\n'
        def retained(name, data):
            path = directory / name
            path.write_bytes(data)
            return {'path': str(path.relative_to(self.root)), 'sha256': hashlib.sha256(data).hexdigest(),
                    'originalPath': name,
                    'source': f'{self.repository.removesuffix(".git")}/blob/{self.revision}/{name}',
                    'gitBlob': hashlib.sha1(b'blob ' + str(len(data)).encode() + b'\0' + data).hexdigest()}
        self.entry = {'name': 'vendor', 'version': '0.1.0', 'repository': self.repository,
                      'revision': self.revision, 'source': self.source, 'declaredLicense': 'MIT',
                      'selectedLicense': 'MIT', 'manifest': retained('vendor.toml', manifest),
                      'files': [retained('LICENSE', license_text)]}
        self.entry['manifestPath'] = 'vendor.toml'
        self.entry['manifestGitBlob'] = self.entry['manifest']['gitBlob']
        self.catalog = {'packages': [], 'toolchains': [], 'gitPackages': [self.entry]}
        self.write_catalog()
        self.write_lock(self.source)

    def write_catalog(self):
        (self.root / 'licenses/dependencies/catalog.json').write_text(json.dumps(self.catalog))

    def write_lock(self, source):
        (self.root / 'Cargo.lock').write_text(f'[[package]]\nname="vendor"\nversion="0.1.0"\nsource="{source}"\n')

    def validate(self):
        with patch.object(MODULE, 'ROOT', self.root), patch.object(MODULE.subprocess, 'check_output', return_value='Cargo.lock\n'):
            return MODULE.validate()

    def test_exact_package_revision_and_license_pass(self):
        self.assertEqual(self.validate(), 1)

    def test_foreign_repository_and_unknown_revision_fail(self):
        for source in [self.source.replace('allowit-hq', 'foreign'), self.source.replace(self.revision, 'b' * 40)]:
            self.write_lock(source)
            with self.assertRaisesRegex(ValueError, 'explicit license coverage'):
                self.validate()

    def test_changed_license_text_fails(self):
        (self.root / self.entry['files'][0]['path']).write_text('changed grant')
        with self.assertRaisesRegex(ValueError, 'hash mismatch'):
            self.validate()

    def test_changed_manifest_license_fails_even_with_updated_hash(self):
        path = self.root / self.entry['manifest']['path']
        path.write_text(path.read_text().replace('MIT', 'Apache-2.0'))
        self.entry['manifest']['sha256'] = hashlib.sha256(path.read_bytes()).hexdigest()
        data = path.read_bytes()
        self.entry['manifestGitBlob'] = hashlib.sha1(b'blob ' + str(len(data)).encode() + b'\0' + data).hexdigest()
        self.entry['manifest']['gitBlob'] = self.entry['manifestGitBlob']
        self.write_catalog()
        with self.assertRaisesRegex(ValueError, 'license differs'):
            self.validate()

    def test_input_source_must_match_the_exact_pinned_repository_path(self):
        self.entry['files'][0]['source'] = self.entry['files'][0]['source'].replace('allowit-hq', 'foreign')
        self.write_catalog()
        with self.assertRaisesRegex(ValueError, 'pinned source path'):
            self.validate()

    def test_source_revision_mismatch_and_path_escape_fail(self):
        self.entry['source'] = self.source.replace(self.revision, 'c' * 40)
        self.write_catalog()
        with self.assertRaisesRegex(ValueError, 'exact canonical'):
            self.validate()
        self.entry['source'] = self.source
        self.entry['manifest']['path'] = '../outside.toml'
        self.write_catalog()
        with self.assertRaisesRegex(ValueError, 'escapes'):
            self.validate()


    def test_wrong_git_package_identity_fails(self):
        lock = self.root / 'Cargo.lock'
        lock.write_text(lock.read_text().replace('name="vendor"', 'name="another-package"'))
        with self.assertRaisesRegex(ValueError, 'explicit license coverage'):
            self.validate()

    def test_changed_or_unknown_registry_identity_fails(self):
        self.catalog['packages'] = [{'name': 'registry', 'version': '1.0.0',
                                     'crateSha256': 'c' * 64, 'files': self.entry['files']}]
        self.write_catalog()
        lock = self.root / 'Cargo.lock'
        original = lock.read_text()
        registry = '\n[[package]]\nname="registry"\nversion="1.0.0"\nsource="registry+https://github.com/rust-lang/crates.io-index"\nchecksum="' + 'c' * 64 + '"\n'
        lock.write_text(original + registry)
        self.assertEqual(self.validate(), 2)
        for changed in [registry.replace('c' * 64, 'd' * 64), registry.replace('version="1.0.0"', 'version="2.0.0"')]:
            lock.write_text(original + changed)
            with self.assertRaisesRegex(ValueError, 'does not cover'):
                self.validate()

    def test_manifest_and_notice_blob_provenance_fail_closed(self):
        self.entry['manifestGitBlob'] = 'd' * 40
        self.write_catalog()
        with self.assertRaisesRegex(ValueError, 'manifest blob'):
            self.validate()
        self.entry['manifestGitBlob'] = self.entry['manifest']['gitBlob']
        self.entry['files'][0]['gitBlob'] = 'd' * 40
        self.write_catalog()
        with self.assertRaisesRegex(ValueError, 'license blob'):
            self.validate()

    def test_manifest_source_path_cannot_be_rebound(self):
        self.entry['manifestPath'] = 'other/Cargo.toml'
        self.write_catalog()
        with self.assertRaisesRegex(ValueError, 'manifest blob'):
            self.validate()

    def test_missing_manifest_empty_notice_and_duplicate_identity_fail(self):
        manifest = self.entry.pop('manifest')
        self.write_catalog()
        with self.assertRaises(KeyError):
            self.validate()
        self.entry['manifest'] = manifest
        files = self.entry['files']
        self.entry['files'] = []
        self.write_catalog()
        with self.assertRaisesRegex(ValueError, 'Empty license'):
            self.validate()
        self.entry['files'] = files
        self.catalog['gitPackages'].append(self.entry)
        self.write_catalog()
        with self.assertRaisesRegex(ValueError, 'Duplicate'):
            self.validate()

    def test_proprietary_manifest_is_preserved_without_a_license_grant(self):
        path = self.root / self.entry['manifest']['path']
        raw = path.read_bytes().replace(b'MIT', b'LicenseRef-Proprietary')
        path.write_bytes(raw)
        self.entry['manifest']['sha256'] = hashlib.sha256(raw).hexdigest()
        self.entry['manifestGitBlob'] = hashlib.sha1(b'blob ' + str(len(raw)).encode() + b'\0' + raw).hexdigest()
        self.entry['declaredLicense'] = 'LicenseRef-Proprietary'
        self.entry.pop('selectedLicense')
        original_mit_notice = self.entry['files']
        self.entry['manifest']['gitBlob'] = self.entry['manifestGitBlob']
        self.entry['files'] = [dict(self.entry['manifest'])]
        self.write_catalog()
        self.assertEqual(self.validate(), 1)
        self.entry['files'] = original_mit_notice
        self.write_catalog()
        with self.assertRaisesRegex(ValueError, 'Proprietary package notices'):
            self.validate()
        self.entry['files'] = [dict(self.entry['manifest']), original_mit_notice[0]]
        self.write_catalog()
        with self.assertRaisesRegex(ValueError, 'Proprietary package notices'):
            self.validate()
        self.entry['files'] = [dict(self.entry['manifest'])]
        self.entry['selectedLicense'] = 'MIT'
        self.write_catalog()
        with self.assertRaisesRegex(ValueError, 'license differs'):
            self.validate()


    def test_mit_package_cannot_substitute_manifest_for_license_notice(self):
        self.entry['files'] = [dict(self.entry['manifest'])]
        self.write_catalog()
        with self.assertRaisesRegex(ValueError, 'original license notice separate'):
            self.validate()


    def test_noncanonical_original_source_paths_fail(self):
        original_notice = dict(self.entry['files'][0])
        for original in ['./LICENSE', 'nested//LICENSE', 'nested/./LICENSE', 'LICENSE/', '']:
            self.entry['files'][0] = dict(original_notice)
            notice = self.entry['files'][0]
            notice['originalPath'] = original
            notice['source'] = f'{self.repository.removesuffix(".git")}/blob/{self.revision}/{original}'
            self.write_catalog()
            with self.assertRaisesRegex(ValueError, 'pinned source path'):
                self.validate()

    def test_manifest_bytes_cannot_masquerade_as_a_license_notice(self):
        self.entry['files'][0] = dict(self.entry['manifest'])
        self.entry['files'][0]['originalPath'] = 'LICENSE'
        self.entry['files'][0]['source'] = f'{self.repository.removesuffix(".git")}/blob/{self.revision}/LICENSE'
        self.write_catalog()
        with self.assertRaisesRegex(ValueError, 'original license notice separate'):
            self.validate()


    def test_manifest_notice_blob_metadata_cannot_drift(self):
        self.entry['manifest']['gitBlob'] = 'd' * 40
        self.write_catalog()
        with self.assertRaisesRegex(ValueError, 'manifest blob'):
            self.validate()


if __name__ == '__main__':
    unittest.main()
