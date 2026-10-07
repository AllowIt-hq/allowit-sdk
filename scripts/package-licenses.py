#!/usr/bin/env python3
"""Validate pinned license inputs and attach them to a binary distribution."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parent.parent


def validate():
    catalog = json.loads((ROOT / 'licenses/dependencies/catalog.json').read_text())
    packages = {(p['name'], p['version'], p['crateSha256']) for p in catalog['packages']}
    tracked = subprocess.check_output(
        ['git', 'ls-files', '--', '*Cargo.lock'], cwd=ROOT, text=True
    ).splitlines()
    for lock in tracked:
        for package in tomllib.loads((ROOT / lock).read_text()).get('package', []):
            source = package.get('source', '')
            if source and not source.startswith('registry+'):
                raise ValueError(f'Non-registry dependency requires explicit license coverage: {lock}: {package["name"]}')
            if source.startswith('registry+'):
                identity = (package['name'], package['version'], package['checksum'])
                if identity not in packages:
                    raise ValueError(f'License catalog does not cover {lock}: {identity[0]} {identity[1]}')
    entries = catalog['packages'] + catalog['toolchains']
    for entry in entries:
        if not entry['files']:
            raise ValueError('Empty license entry')
        for notice in entry['files']:
            path = ROOT / notice['path']
            if not path.resolve().is_relative_to((ROOT / 'licenses').resolve()):
                raise ValueError('License path escapes licenses directory')
            if hashlib.sha256(path.read_bytes()).hexdigest() != notice['sha256']:
                raise ValueError(f'License text hash mismatch: {notice["path"]}')
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
