"""Organize a user's local TrayBuddy source and converted package for Studio.

Only writes a new output directory. Never ships copyrighted fixture assets in git.
Requires Pillow for lossless extraction of the first idle frame.
"""
import argparse, csv, hashlib, io, json, zipfile
from pathlib import Path
from PIL import Image

def sha(data): return hashlib.sha256(data).hexdigest()
def write_json(path, value): path.write_text(json.dumps(value,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')

def main():
    ap=argparse.ArgumentParser();ap.add_argument('--source',required=True);ap.add_argument('--converted',required=True);ap.add_argument('--output',required=True);args=ap.parse_args()
    source,converted,out=map(Path,(args.source,args.converted,args.output))
    if out.exists(): raise SystemExit('Output must be a new directory to preserve previous materials.')
    out.mkdir(parents=True)
    inventory=[]
    with zipfile.ZipFile(source) as original,zipfile.ZipFile(converted) as archive:
        hashes={sha(original.read(name)):name for name in original.namelist() if not name.endswith('/')}
        manifest=json.loads(archive.read('manifest.json'));model=json.loads(archive.read(manifest['avatar']['descriptor']))
        group={'image':'01-图像图集','audio':'02-固定音声','text':'03-固定文字'}
        for key,asset in manifest['assets'].items():
            data=archive.read(asset['path']);kind=asset['kind'];relative=Path(group.get(kind,'04-其他'))/(key+Path(asset['path']).suffix)
            # Stable IDs become filenames; the workbench imports them verbatim.
            if '/' in key or '\\' in key: raise ValueError('Unexpected asset ID')
            target=out/relative;target.parent.mkdir(exist_ok=True);target.write_bytes(data)
            found=hashes.get(sha(data),'中文文字从原包语言表展开为UTF-8')
            if kind in ('image','audio') and sha(data) not in hashes: raise ValueError('Image/audio is not byte-identical to the provided source')
            inventory.append({'素材编号':key,'类型':kind,'导入文件':relative.as_posix(),'字节':len(data),'SHA256':sha(data),'原包来源':found})
        write_json(out/'04-Ema完整动作清单.json',model)
        idle=model['clips'][model['baseModes']['idle']['loop']];image=Image.open(io.BytesIO(archive.read(manifest['assets'][idle['asset']]['path'])))
        l=idle['layout'];frame=(0,0,l.get('cellWidth',image.width),l.get('cellHeight',image.height))
        static=out/'00-先做静态角色';static.mkdir();image.crop(frame).save(static/'image.static-idle.png')
        (out/'05-人格与称呼.txt').write_text('角色名称：艾玛（Ema）\n用户称呼：你（可自行修改）\n\n下面是本次制作练习的人格示例，并非原作者官方设定，可自行调整：\n你是名叫艾玛的桌面伙伴。语气温柔、活泼，关心用户但不过度打扰。自然使用简短中文交流。只依据用户明确提供的信息表达关怀，不臆测用户正在做什么。动作通过角色模型表达，不把系统状态或内部工具信息当作正文。\n',encoding='utf-8')
        presets=manifest.get('pluginPresets',[])
        pointer=json.loads((Path(__file__).resolve().parent.parent/'plugins/builtin/pointer-interaction.json').read_text(encoding='utf-8'))['manifest']
        config=pointer['defaults'];config['start']='00:00';config['end']='00:00';config['cooldownSeconds']=1
        config['response']={'mode':'state-specific','stateResponses':[{'id':'idle-click','base':'idle','response':{'mode':'fixed','variants':[{'id':'hello-a','textAsset':'text.morning','audioAsset':'voice.morning','animationAsset':'wave'},{'id':'hello-b','textAsset':'text.noon','audioAsset':'voice.noon','animationAsset':'wink'}]}},{'id':'music-click','base':'music','response':{'mode':'fixed','variants':[{'id':'music-a','text':'一起听会儿音乐吧。','animationAsset':'wink'}]}}]}
        presets.append({'presetId':'ema-click','pluginId':pointer['pluginId'],'configVersion':'1.0','configMode':'complete','enabled':True,'priority':'background','config':config})
        write_json(out/'06-Ema插件预设清单.json',presets)
        # Optional nine-slice starter is an original border bitmap, kept separate
        # from the default pure-color theme; the user chooses whether to adopt it.
        info=out/'07-原始语言对照';info.mkdir()
        for name in original.namelist():
            if '/text/' in name and name.endswith('.json'):
                parts=Path(name).parts[-2:];target=info.joinpath(*parts);target.parent.mkdir(exist_ok=True);target.write_bytes(original.read(name))
        with (out/'素材清单.csv').open('w',encoding='utf-8-sig',newline='') as f:
            writer=csv.DictWriter(f,fieldnames=list(inventory[0]));writer.writeheader();writer.writerows(inventory)
        write_json(out/'来源与校验.json',{'source':source.name,'sourceSHA256':sha(source.read_bytes()),'converted':converted.name,'convertedSHA256':sha(converted.read_bytes()),'imagesAndAudio':'与本机原包字节一致；静态待机图仅无损提取第一帧','text':'固定文字由原包中文语言表展开；原始各语言表保留对照','copyright':'原素材归原作者；仅供用户本机制作验收，不随公共源码或Release分发','counts':{kind:sum(a['类型']==kind for a in inventory) for kind in group},'clips':len(model['clips'])})
        clips=['# Ema 动作清单\n','逻辑画布：%s × %s。脚底锚点 %s；气泡锚点 %s。\n'%(model['logicalSize']['width'],model['logicalSize']['height'],model['anchors']['feet'],model['anchors']['bubble'])]
        for name,c in model['clips'].items(): clips.append('- `%s` → `%s`；%s；每帧 %s 毫秒；%s。'%(name,c['asset'],json.dumps(c['layout'],ensure_ascii=False),c['frameDurationMs'],c['direction']))
        clips+=['\n基础模式：`'+json.dumps(model['baseModes'],ensure_ascii=False)+'`','\n语义映射：`'+json.dumps(model.get('semanticCues',{}),ensure_ascii=False)+'`']
        (out/'动作参数说明.md').write_text('\n'.join(clips)+'\n',encoding='utf-8')
        (out/'先读我.md').write_text('# Ema 本机制作素材\n\n这是原始素材与参考参数，不是现成角色包或工作台项目。请按候选版随附的《Ema-工作台完整制作教程》从新建角色开始。\n\n先用 00 中的静态图片完成最小角色，再导入 01、02、03 文件夹中的全部文件（文件名就是素材编号），并导入 04 动作清单。05 是可调整的人格示例；06 是可选的完整预设合并清单，建议先手工创建一项点击互动再导入对照。07 保留原包语言表。\n\n音乐/全屏姿态来自插件规则，未启动相关插件时保持 idle。普通短素材仍按全局最少阅读时间显示。所有 Ema 文件仅留本机，不提交公共仓库。\n',encoding='utf-8')
    print(json.dumps({'assets':len(inventory),'clips':len(model['clips']),'output':str(out)},ensure_ascii=False))

if __name__=='__main__':main()
