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


def checked_file(entry):
    path = ROOT / entry['path']
    if not path.resolve().is_relative_to((ROOT / 'licenses').resolve()):
        raise ValueError('License path escapes licenses directory')
    data = path.read_bytes()
    if hashlib.sha256(data).hexdigest() != entry['sha256']:
        raise ValueError(f'License input hash mismatch: {entry["path"]}')
    return data


def checked_git_input(entry, repository, revision):
    original = entry['originalPath']
    if (not isinstance(original, str) or '\\' in original
            or original.startswith('/') or '..' in Path(original).parts
            or entry['source'] != f'{repository.removesuffix(".git")}/blob/{revision}/{original}'):
        raise ValueError('Git license input does not name its pinned source path')
    return checked_file(entry)


def validate():
    catalog = json.loads((ROOT / 'licenses/dependencies/catalog.json').read_text())
    packages = {(p['name'], p['version'], p['crateSha256']) for p in catalog['packages']}
    git_packages = set()
    for entry in catalog.get('gitPackages', []):
        revision = entry['revision']
        repository = entry['repository']
        source = f'git+{repository}?rev={revision}#{revision}'
        if (not re.fullmatch(r'[0-9a-f]{40}', revision)
                or not re.fullmatch(r'https://github\.com/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+\.git', repository)
                or entry['cargoSource'] != source):
            raise ValueError('Git license coverage requires an exact canonical repository and revision')
        package = tomllib.loads(checked_git_input(entry['manifest'], repository, revision).decode())['package']
        if (package['name'] != entry['name'] or package['version'] != entry['version']
                or package.get('license') != 'MIT' or entry['declaredLicense'] != 'MIT'
                or entry['selectedLicense'] != 'MIT'):
            raise ValueError('Pinned Git package identity or MIT license differs from its manifest')
        identity = (entry['name'], entry['version'], source)
        if identity in git_packages:
            raise ValueError('Duplicate Git package license identity')
        git_packages.add(identity)
        for notice in entry['files']:
            checked_git_input(notice, repository, revision)
    tracked = subprocess.check_output(
        ['git', 'ls-files', '--', '*Cargo.lock'], cwd=ROOT, text=True
    ).splitlines()
    for lock in tracked:
        for package in tomllib.loads((ROOT / lock).read_text()).get('package', []):
            source = package.get('source', '')
            if source and not source.startswith('registry+') and (package['name'], package['version'], source) not in git_packages:
                raise ValueError(f'Non-registry dependency requires explicit license coverage: {lock}: {package["name"]}')
            if source.startswith('registry+'):
                identity = (package['name'], package['version'], package['checksum'])
                if identity not in packages:
                    raise ValueError(f'License catalog does not cover {lock}: {identity[0]} {identity[1]}')
    entries = catalog['packages'] + catalog.get('gitPackages', []) + catalog['toolchains']
    for entry in entries:
        if not entry['files']:
            raise ValueError('Empty license entry')
        for notice in entry['files']:
            checked_file(notice)
    return len(packages) + len(git_packages)


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
    print(f'License catalog verified: {count} registry and Git package versions')


if __name__ == '__main__':
    main()
