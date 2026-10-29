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
        self.repository = 'https://github.com/AllowIt-hq/example.git'
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
                    'source': f'{self.repository.removesuffix(".git")}/blob/{self.revision}/{name}'}
        self.entry = {'name': 'vendor', 'version': '0.1.0', 'repository': self.repository,
                      'revision': self.revision, 'cargoSource': self.source, 'declaredLicense': 'MIT',
                      'selectedLicense': 'MIT', 'manifest': retained('vendor.toml', manifest),
                      'files': [retained('LICENSE', license_text)]}
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
        for source in [self.source.replace('AllowIt-hq', 'foreign'), self.source.replace(self.revision, 'b' * 40)]:
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
        self.write_catalog()
        with self.assertRaisesRegex(ValueError, 'MIT license differs'):
            self.validate()

    def test_input_source_must_match_the_exact_pinned_repository_path(self):
        self.entry['files'][0]['source'] = self.entry['files'][0]['source'].replace('AllowIt-hq', 'foreign')
        self.write_catalog()
        with self.assertRaisesRegex(ValueError, 'pinned source path'):
            self.validate()

    def test_source_revision_mismatch_and_path_escape_fail(self):
        self.entry['cargoSource'] = self.source.replace(self.revision, 'c' * 40)
        self.write_catalog()
        with self.assertRaisesRegex(ValueError, 'exact canonical'):
            self.validate()
        self.entry['cargoSource'] = self.source
        self.entry['manifest']['path'] = '../outside.toml'
        self.write_catalog()
        with self.assertRaisesRegex(ValueError, 'escapes'):
            self.validate()


if __name__ == '__main__':
    unittest.main()
