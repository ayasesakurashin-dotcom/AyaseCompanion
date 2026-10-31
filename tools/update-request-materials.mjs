// Rebuild the shared fixed/fallback fields and every embedded copy of that contract.
import {readFileSync,writeFileSync} from 'node:fs';
const read=p=>JSON.parse(readFileSync(p,'utf8'));
const save=(p,v)=>writeFileSync(p,JSON.stringify(v,null,2)+'\n');
const request=read('contracts/request.schema.json');
for(const key of ['variants','fallback']){
  const fields=request.properties[key].items.properties;
  for(const name of ['text','textAsset','audioAsset','cue']){
    const literal=name==='text';
    fields[name]={title:({text:'固定文字（支持多段）',textAsset:'文字素材编号',audioAsset:'音声素材编号',cue:'语义动作编号'})[name],
      description:literal?'每个列表条目独立分页和阅读；标点与换行保留。整组属于同一任务。':'按顺序播放，编号可用逗号或换行分隔，也可使用 JSON 数组；最多32条。',
      anyOf:[{type:'null'},{type:'string',maxLength:literal?12000:8192},{type:'array',maxItems:32,items:{type:'string',maxLength:literal?12000:128}}]};
  }
}
save('contracts/request.schema.json',request);
const files=['plugins/builtin/startup.json','plugins/examples/nightly-rest/schemas/config.schema.json'];
function replace(v){if(!v||typeof v!=='object')return v;if(v.properties?.mode?.enum?.includes('generated-with-fallback')&&v.properties?.variants)return request;for(const k of Object.keys(v))v[k]=replace(v[k]);return v;}
for(const file of files)save(file,replace(read(file)));
