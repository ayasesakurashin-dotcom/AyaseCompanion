"""Create private authoring materials from a user's existing Ema package and atlas.

Copies the provided artwork unchanged. The simple halo images are generated test
fixtures. The output directory must be new; nothing is written into the source.
"""
import argparse
import hashlib
import io
import json
import zipfile
from pathlib import Path
from PIL import Image, ImageDraw


def encode(value):
    return (json.dumps(value, ensure_ascii=False, indent=2) + '\n').encode('utf-8')


def digest(data):
    return hashlib.sha256(data).hexdigest()


def pack(files, manifest):
    files = dict(files)
    files.pop('manifest.json', None)
    manifest = json.loads(json.dumps(manifest))
    manifest['integrity'] = {name: digest(data) for name, data in files.items()}
    files['manifest.json'] = encode(manifest)
    target = io.BytesIO()
    with zipfile.ZipFile(target, 'w', zipfile.ZIP_DEFLATED) as z:
        for name, data in files.items():
            z.writestr(name, data)
    return target.getvalue()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--base', required=True)
    ap.add_argument('--border', required=True)
    ap.add_argument('--output', required=True)
    args = ap.parse_args()
    out = Path(args.output)
    if out.exists():
        raise SystemExit('Choose a new output directory.')
    border = Path(args.border).read_bytes()
    if Image.open(io.BytesIO(border)).size != (1592, 355):
        raise SystemExit('Expected the supplied 8 by 5 Ema atlas (1592 by 355).')
    with zipfile.ZipFile(args.base) as z:
        files = {name: z.read(name) for name in z.namelist() if not name.endswith('/')}
    manifest = json.loads(files['manifest.json'])
    model_path = manifest['avatar']['descriptor']
    model = json.loads(files[model_path])
    out.mkdir(parents=True)
    (out / '00-Ema基础包.ayase').write_bytes(Path(args.base).read_bytes())
    assets = out / '01-装饰素材'
    assets.mkdir()
    (assets / 'image.ema-border.webp').write_bytes(border)
    manifest['packageId'] = 'local.ema.decorations.demo'
    manifest['displayName'] = 'Ema 装饰扩展示例'
    manifest['version'] = '0.4.1'
    manifest['assets']['image.ema-border'] = {'kind': 'image', 'path': 'assets/decorations/border.webp'}
    files['assets/decorations/border.webp'] = border
    for name, color in [('pink', (238, 165, 191)), ('blue', (139, 200, 239))]:
        im = Image.new('RGBA', (256, 256))
        draw = ImageDraw.Draw(im)
        draw.ellipse((10, 10, 246, 246), fill=(*color, 18), outline=(*color, 180), width=9)
        draw.ellipse((26, 26, 230, 230), outline=(*color, 90), width=3)
        stream = io.BytesIO()
        im.save(stream, format='PNG')
        key = f'image.demo-halo-{name}'
        path = f'assets/decorations/halo-{name}.png'
        files[path] = stream.getvalue()
        manifest['assets'][key] = {'kind': 'image', 'path': path}
        (assets / f'{key}.png').write_bytes(stream.getvalue())
    model['clips']['decoration.ema-border'] = {
        'asset': 'image.ema-border',
        'layout': {'type': 'grid', 'columns': 8, 'rows': 5, 'frameCount': 40, 'cellWidth': 199, 'cellHeight': 71},
        'frameDurationMs': 200, 'direction': 'forward'
    }
    def layer(id, side, source, width, anchor, pivot, offset, states):
        return {'id': id, 'name': id, 'side': side, 'order': 0, 'enabled': True, 'source': source,
                'anchor': anchor, 'pivot': pivot, 'offset': offset, 'width': width, 'height': None,
                'keepAspect': True, 'scale': 1, 'speed': 1, 'states': states}
    model['decorationVersion'] = '1.0'
    model['decorations'] = [
        layer('待机与安静光环', 'background', {'type': 'image', 'asset': 'image.demo-halo-pink'}, 230, {'x': .5, 'y': .5}, {'x': .5, 'y': .5}, {'x': 0, 'y': -6}, ['idle', 'quiet']),
        layer('音乐光环', 'background', {'type': 'image', 'asset': 'image.demo-halo-blue'}, 240, {'x': .5, 'y': .5}, {'x': .5, 'y': .5}, {'x': 0, 'y': -6}, ['music']),
        layer('前景花环', 'foreground', {'type': 'clip', 'clip': 'decoration.ema-border'}, 260, dict(model['anchors']['feet']), {'x': .5, 'y': .5}, {'x': 0, 'y': -8}, None)
    ]
    files[model_path] = encode(model)
    (out / '02-完整模型动作清单.json').write_bytes(encode(model))
    (out / '03-装饰参数参考.json').write_bytes(encode({'decorationVersion': '1.0', 'decorations': model['decorations']}))
    (out / '04-Ema装饰完成示例.ayase').write_bytes(pack(files, manifest))
    guide = Path(__file__).resolve().parent.parent / 'docs/09-decoration-authoring-guide.md'
    (out / '05-制作操作说明.md').write_bytes(guide.read_bytes())
    inventory = [{'file': p.relative_to(out).as_posix(), 'sha256': digest(p.read_bytes()), 'bytes': p.stat().st_size} for p in out.rglob('*') if p.is_file()]
    (out / 'SHA256.json').write_bytes(encode({'sourceBorderSHA256': digest(border), 'files': inventory}))
    print('Created decoration authoring materials. Source atlas copied unchanged.')


if __name__ == '__main__':
    main()
