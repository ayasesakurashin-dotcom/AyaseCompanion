import {setupP2} from '../src/p2.ts';
import type {Snapshot,Json} from '../src/types.ts';
import type {invoke} from '@tauri-apps/api/core';
import '../src/style.css';
if(!import.meta.env.DEV)throw Error('Development-only synthetic preview');
const s={instance:{id:'synthetic-a',name:'预览伙伴 A'},package:{hash:'synthetic-package'},scheduler:{tasks:[]},ai:{service:{settings:{chat:{enabled:true,endpoint:'https://example.org/v1',model:'example-model'},voice:{enabled:true,endpoint:'https://api.fish.audio/v1',model:'s2-pro'},memory:{enabled:false,endpoint:'http://127.0.0.1:8888',model:''},voiceKind:'fish',timeoutSeconds:60,maxChars:20}},profile:{characterName:'预览伙伴',userAddress:'你',persona:'用自然、简短的话语交流。',language:'简体中文',voiceId:'synthetic-voice',readLanguage:'日语',voiceEnabled:true,syncMemory:false,presentationPolicy:'text-first',sendPolicy:'wait-current'},voiceSamples:[{id:'voice.greeting',durationMs:4200,transcript:'你好，今天也很高兴见到你。'},{id:'voice.encouragement',durationMs:5800,transcript:'休息一下，慢慢来就好。'}],voiceModels:[],audioAssets:{'voice.greeting':true,'voice.encouragement':true},memory:{messages:[{role:"assistant",source:"chat",status:"completed",created:"2026-01-01 12:00:00",content:"# 测试摘要\n\n**进展**",displayContent:"测试摘要\n\n项目进展：\n1. 已完成第一项。\n2. 第二项正在处理。\n\n代码示例：\n  result = 2 * 3"},{role:"user",source:"chat",status:"submitted",created:"2026-01-01 12:00:00",content:"**这段用户原文保留标记**"}],epoch:0,pendingSync:0}}} as unknown as Snapshot;
const records:Record<string,Json[]>={};
let update:(s:Snapshot)=>void;
const mock=async(command:string,args:Json={})=>{
  await new Promise(r=>setTimeout(r,500));
  if(command==='service_probe')return {message:'模拟连接已响应；未请求真实服务。',elapsedMs:500};
  if(command==='create_voice'){
    const record={instanceId:args.instanceId,endpoint:args.connection.provider.endpoint,id:'fixture_voice_1',title:args.title,state:'created',visibility:'private'};
    (records[args.instanceId]??=[]).push(record);s.ai!.voiceModels=records[s.instance!.id]??[];update(s);return record;
  }
  if(command==='refresh_voice'){const record=records[args.instanceId]?.find(r=>r.id===args.id);if(record)record.state='trained';update(s);return record;}
  if(command==='preview_voice')return {taskId:1,message:'模拟试听已排队；此预览不播放音频。'};
  if(command==='save_profile'){if(s.instance!.id===args.instanceId)s.ai!.profile=args.profile;update(s);}
  if(command==='save_services'){s.ai!.service.settings=args.settings;update(s);}
};
update=setupP2(document.querySelector('#app')!,{invoke:mock as typeof invoke});update(s);
document.querySelector('[data-panel="ai"]')!.removeAttribute('hidden');
const switcher=document.createElement('button');switcher.textContent='切换模拟伙伴 A / B';switcher.onclick=()=>{s.instance={id:s.instance!.id==='synthetic-a'?'synthetic-b':'synthetic-a',name:s.instance!.id==='synthetic-a'?'预览伙伴 B':'预览伙伴 A'};s.ai!.voiceModels=records[s.instance.id]??[];update(s);};document.querySelector('nav')!.append(switcher);
