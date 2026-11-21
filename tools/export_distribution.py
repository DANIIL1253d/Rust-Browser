"""Export an allowlisted source tree and a checksum-verified unpacked package."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil

DEVELOPMENT = {'.git', '.build', 'target', 'work', 'releases', 'graphify-out', '__pycache__'}
PRIVATE = {'profile.json', 'passwords.dat', 'connections.dat', 'cookies', 'history', 'login data', 'local state', 'auth.json'}
LOCAL_REPORTS = {'continue.md', 'session-report.md', 'build-report.md', 'build-manifest.json', 'community-report.md', 'community-improvements.md', 'private-repository.md', 'browser-plan.md', 'browser-design.md', 'next-improvements.md'}


def selected_file(root, relative):
    if not isinstance(relative, str) or not relative or '\\' in relative:
        raise ValueError('Invalid manifest path')
    parts = relative.split('/')
    for part in parts:
        if (not part or part in ('.', '..') or part.rstrip(' .') != part or
                re.search(r'[<>:"|?*\x00-\x1f]', part) or
                re.match(r'^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)', part, re.I) or
                part.casefold() in DEVELOPMENT | PRIVATE | LOCAL_REPORTS or
                part.casefold().endswith(('.log', '.sqlite', '.db', '.csv', '.key', '.pfx', '.pyc'))):
            raise ValueError('Private/development or unsafe path: ' + relative)
    file = root.joinpath(*parts)
    if not file.resolve().is_relative_to(root):
        raise ValueError('Manifest path escapes input: ' + relative)
    if not file.is_file():
        raise FileNotFoundError(relative)
    return file


def digest(file):
    with file.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def export(source, package, output):
    source, package, output = (Path(p).resolve() for p in (source, package, output))
    if output == source or output.is_relative_to(source) or output == package or output.is_relative_to(package):
        raise ValueError('Output must be separate from both inputs')
    if output.exists():
        raise FileExistsError('Output already exists; it was preserved')
    overlay = json.loads((source / 'BROWSER-OVERLAY.json').read_text(encoding='utf-8-sig'))
    manifest = json.loads((package / 'candidate-manifest.json').read_text(encoding='utf-8-sig'))
    files = {}
    for relative in overlay['files']:
        files['source/' + relative] = selected_file(source, relative)
    for relative, expected in manifest['sha256'].items():
        if not relative.startswith('app/') or not re.fullmatch('[0-9a-f]{64}', expected):
            raise ValueError('Invalid application manifest entry')
        file = selected_file(package, relative)
        if digest(file) != expected:
            raise ValueError('Application checksum mismatch: ' + relative)
        files[relative] = file
    if 'app/RustBrowser.exe' not in files or not overlay['files']:
        raise ValueError('Application or sources are missing')
    if len({name.casefold() for name in files}) != len(files):
        raise ValueError('Case-insensitive destination collision')
    files['Start-Browser.cmd'] = selected_file(source, 'Start-Browser.cmd')
    output.mkdir(parents=True)
    for relative, file in files.items():
        target = output / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(file, target)
        if digest(target) != manifest['sha256'].get(relative, digest(file)):
            raise OSError('Copy verification failed: ' + relative)
    status = ('Экспериментальная сборка для тестирования. Полная изоляция веб-контента '
              'в Windows и совместимость сайтов ещё не подтверждены.\n\n'
              if not manifest.get('stable_release', False) else '')
    (output / 'README.md').write_text(
        '# Rust Browser\n\nЗапуск: **Start-Browser.cmd** или **app/RustBrowser.exe**.\n\n'
        'В `app` находятся приложение, ресурсы и лицензии. В `source` — исходники, '
        'тесты и команды сборки; начни с `source/README.md`. Servo скачивается на '
        'закреплённой ревизии при первой сборке исходников.\n\n' + status +
        'Личные данные и локальные материалы разработки в комплект не входят.\n', encoding='utf-8')
    checksums = {p.relative_to(output).as_posix(): digest(p) for p in output.rglob('*') if p.is_file()}
    (output / 'SHA256SUMS.txt').write_text(''.join(f'{value}  {name}\n' for name, value in sorted(checksums.items())), encoding='utf-8')
    return {'application_files': len(manifest['sha256']), 'source_files': len(overlay['files']), 'output': str(output)}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--package', required=True, type=Path, help='Unpacked candidate with candidate-manifest.json')
    parser.add_argument('--output', required=True, type=Path, help='New, separate destination folder')
    args = parser.parse_args()
    print(json.dumps(export(Path(__file__).resolve().parents[1], args.package, args.output), ensure_ascii=False))
