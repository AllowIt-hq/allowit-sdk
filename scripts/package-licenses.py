#!/usr/bin/env python3
"""Validate pinned license inputs and attach them to a binary distribution."""
import argparse
import hashlib
import json
import re
from pathlib import Path
import shutil
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parent.parent


def validate():
    catalog = json.loads((ROOT / 'licenses/dependencies/catalog.json').read_text())
    packages = {(p['name'], p['version'], p['crateSha256']) for p in catalog['packages']}
    git_entries = catalog.get('gitPackages', [])
    git_packages = set()
    for entry in git_entries:
        revision = entry['revision']
        repository = entry['repository']
        if not re.fullmatch(r'[0-9a-f]{40}', revision) or not re.fullmatch(r'https://github\.com/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+(?:\.git)?', repository):
            raise ValueError('Invalid exact Git license source')
        expected = 'git+' + repository + '?rev=' + revision + '#' + revision
        if entry['source'] != expected or not re.fullmatch(r'[0-9a-f]{40}', entry['manifestGitBlob']):
            raise ValueError('Git license source does not match its exact revision')
        identity = (entry['name'], entry['version'], entry['source'])
        if identity in git_packages:
            raise ValueError('Duplicate Git license identity')
        git_packages.add(identity)
        for notice in entry['files']:
            original = Path(notice['originalPath'])
            if original.is_absolute() or '..' in original.parts:
                raise ValueError('Git license original path escapes repository')
            if notice['source'] != repository.removesuffix('.git') + '/blob/' + revision + '/' + original.as_posix():
                raise ValueError('Git license notice source differs from its revision')
    tracked = subprocess.check_output(
        ['git', 'ls-files', '--', '*Cargo.lock'], cwd=ROOT, text=True
    ).splitlines()
    for lock in tracked:
        for package in tomllib.loads((ROOT / lock).read_text()).get('package', []):
            source = package.get('source', '')
            if source and not source.startswith('registry+'):
                if (package['name'], package['version'], source) not in git_packages:
                    raise ValueError(f'Non-registry dependency requires explicit exact license coverage: {lock}: {package["name"]}')
            if source.startswith('registry+'):
                identity = (package['name'], package['version'], package['checksum'])
                if identity not in packages:
                    raise ValueError(f'License catalog does not cover {lock}: {identity[0]} {identity[1]}')
    entries = catalog['packages'] + catalog['toolchains'] + git_entries
    for entry in entries:
        if not entry['files']:
            raise ValueError('Empty license entry')
        for notice in entry['files']:
            path = ROOT / notice['path']
            if not path.resolve().is_relative_to((ROOT / 'licenses').resolve()):
                raise ValueError('License path escapes licenses directory')
            raw = path.read_bytes()
            if hashlib.sha256(raw).hexdigest() != notice['sha256']:
                raise ValueError(f'License text hash mismatch: {notice["path"]}')
            if entry in git_entries:
                blob = hashlib.sha1(b'blob ' + str(len(raw)).encode() + b'\0' + raw).hexdigest()
                if notice.get('gitBlob') != blob:
                    raise ValueError('Git license blob identity differs from retained text')
    return len(packages)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('destination', nargs='?', help='Artifact directory; omit to validate only')
    args = parser.parse_args()
    count = validate()
    if args.destination:
        destination = Path(args.destination).resolve()
        if destination == ROOT or destination.is_relative_to(ROOT / 'licenses') or ROOT.is_relative_to(destination):
            raise ValueError('Destination must be an artifact directory, not the source root, its parents or the licenses directory')
        destination.mkdir(parents=True, exist_ok=True)
        for name in ('LICENSE', 'THIRD_PARTY_NOTICES.md'):
            shutil.copyfile(ROOT / name, destination / name)
        shutil.copytree(ROOT / 'licenses', destination / 'licenses', dirs_exist_ok=True)
    print(f'License catalog verified: {count} registry package versions')


if __name__ == '__main__':
    main()
