// Rebuild the shared fixed/fallback fields and every embedded copy of that contract.
import {readFileSync,writeFileSync} from 'node:fs';
const read=p=>JSON.parse(readFileSync(p,'utf8'));
const save=(p,v)=>writeFileSync(p,JSON.stringify(v,null,2)+'\n');
const request=read('contracts/request.schema.json');
// Refresh the bounded leaf schema before embedding it in state-specific routes.
delete request.properties.stateResponses;
request.properties.mode.enum=['fixed','generated','generated-with-fallback'];
request.properties.mode.title='回复来源：fixed 固定 / generated 生成 / generated-with-fallback 后备';
for(const key of ['variants','fallback']){
  const fields=request.properties[key].items.properties;
  for(const name of ['text','textAsset','audioAsset','cue']){
    const literal=name==='text';
    fields[name]={title:({text:'固定文字（支持多段）',textAsset:'文字素材编号',audioAsset:'音声素材编号',cue:'语义动作编号'})[name],
      description:literal?'每个列表条目独立分页和阅读；标点与换行保留。整组属于同一任务。':'按顺序播放，编号可用逗号或换行分隔，也可使用 JSON 数组；最多32条。',
      anyOf:[{type:'null'},{type:'string',maxLength:literal?12000:8192},{type:'array',maxItems:32,items:{type:'string',maxLength:literal?12000:128}}]};
  }
  fields.animationAsset={...structuredClone(fields.cue),title:'动画素材或片段编号',description:'直接选择当前模型的动画片段，或唯一对应一个片段的动画素材编号。支持逗号/换行或数组；与语义动作并存时先播放语义动作，再播放本列表。'};
  fields.readMs={...fields.readMs,title:'每页阅读时间（毫秒，不低于全局下限）',description:'可以延长每页停留，不能缩短设置中的每页最少阅读时间。完整显现后才开始计时。'};
}
request.properties.animationAsset=structuredClone(request.properties.variants.items.properties.animationAsset);
const leaf=structuredClone(request);
request.properties.mode.enum.push('state-specific');
request.properties.mode.title='回复来源：fixed 固定 / generated 生成 / generated-with-fallback 后备 / state-specific 按状态';
request.properties.stateResponses={type:'array',title:'基础状态与回复',description:'专用状态优先，未配置时仅使用通用（null）；两者皆无则不回应。每个状态最多一项。',maxItems:4,items:{type:'object',additionalProperties:false,required:['id','base','response'],properties:{id:{type:'string',minLength:1,maxLength:128},base:{enum:[null,'idle','music','quiet'],title:'前置基础状态（通用/idle/music/quiet）',default:null},response:{...leaf,title:'此状态的回复'}}}};
save('contracts/request.schema.json',request);
const files=['plugins/builtin/startup.json','plugins/examples/nightly-rest/schemas/config.schema.json'];
function replace(v){if(!v||typeof v!=='object')return v;if(v.properties?.mode?.enum?.includes('generated-with-fallback')&&v.properties?.variants)return request;for(const k of Object.keys(v))v[k]=replace(v[k]);return v;}
for(const file of files)save(file,replace(read(file)));
