"""Prepare local-only assets for the browser regression page from a validated .ayase."""
import json, sys, zipfile, subprocess
from pathlib import Path, PurePosixPath
root = Path(__file__).resolve().parent.parent
package = Path(sys.argv[1]).resolve()
subprocess.run([str(root / 'target/debug/ayase-pack.exe'), 'validate', str(package)], check=True)
dest = root / 'local-assets/browser-preview'
dest.mkdir(parents=True, exist_ok=True)
with zipfile.ZipFile(package) as archive:
    manifest = json.loads(archive.read('manifest.json'))
    model = json.loads(archive.read(manifest['avatar']['descriptor']))
    assets = {}
    for key, entry in manifest['assets'].items():
        if entry['kind'] != 'image': continue
        relative = PurePosixPath(entry['path'])
        if relative.is_absolute() or '..' in relative.parts or '\\' in entry['path']: raise ValueError('Unsafe asset path')
        output = dest.joinpath(*relative.parts)
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_bytes(archive.read(entry['path']))
        assets[key] = '/@fs/' + output.as_posix()
fixture = {'model':model,'manifest':manifest,'assetPaths':assets}
output = root / 'apps/desktop/__test__/local-fixture.json'
output.parent.mkdir(parents=True, exist_ok=True)
output.write_text(json.dumps(fixture, ensure_ascii=False), encoding='utf-8')
print('Local preview fixture ready (git-ignored).')
