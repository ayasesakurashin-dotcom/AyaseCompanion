// Development layout fixture only. Native scheduler correctness is covered by
// isolated Rust/WebView receipt tests, not this simulated bridge.
import {setupStudio} from '../src/studio.ts';
import sprite from '../../../contracts/sprite.schema.json';
import theme from '../../../contracts/theme.schema.json';
import request from '../../../contracts/request.schema.json';
import rules from '../../../contracts/rules.schema.json';
import plugin from '../../../plugins/examples/nightly-rest/plugin.json';
import schema from '../../../plugins/examples/nightly-rest/schemas/config.schema.json';
import {themeDefaults} from '../src/studio.ts';
import '../src/style.css';
if(!import.meta.env.DEV)throw Error('Development-only preview');
const fixture=await fetch('./local-fixture.json').then(r=>r.json());
const documentValue={format:'ayase.project',formatVersion:'1.0',kind:'character',revision:0,manifest:{...fixture.manifest,displayName:'Ema · 制作预览'},model:fixture.model,configSchema:null,rules:null,theme:themeDefaults()};
let current:any={id:'preview-project',document:documentValue,assets:fixture.manifest.assets,assetPaths:fixture.assetPaths,files:[]};let serial=0;
let scheduler:any={active:null,tasks:[],desiredBase:'idle',appliedBase:'idle',suppressed:false,muted:false,log:[],claims:[]};
const state=()=>({projects:[{id:current.id,name:current.document.manifest.displayName,kind:current.document.kind}],current,plugins:[{id:plugin.pluginId,name:plugin.displayName,schema,defaults:plugin.defaults,configVersion:'1.0',priority:'normal'}],contracts:{sprite,theme,request,rules},preview:{projectId:current.id,revision:current.document.revision,model:fixture.model,assetPaths:fixture.assetPaths,theme:current.document.theme??themeDefaults(),zoom:1,scheduler},notice:''});
const bridge={invoke:async(name:string,args:any)=>{if(name==='studio_snapshot')return state();if(name==='studio_ack'){if(!scheduler.active||scheduler.active.id!==args.id)return false;if(args.event==='started')scheduler.active.status='presenting';else{scheduler.active.channels[args.channel]='completed';if(Object.values(scheduler.active.channels).every(v=>v==='completed')){scheduler.active.status='completed';scheduler.active=null;}}return true;}const p=args.payload;if(args.action==='save'){current.document=structuredClone(p.document);current.document.revision++;return true;}if(args.action==='preview'){scheduler.appliedBase=p.base;const text=p.text??'已经很晚了，记得稍微休息。';const task={id:++serial,generation:serial,instanceId:'studio:'+current.id,owner:'studio.preview',priority:'user',status:'starting',plan:{text},channels:{text:'pending'}};scheduler.active=task;scheduler.tasks.push(task);state().preview.zoom=p.zoom;return {taskId:serial};}if(args.action==='stop'){scheduler.active=null;return true;}if(args.action==='validate')return {report:{kind:current.document.kind,displayName:current.document.manifest.displayName,warnings:[]},files:[]};return true;},convertFileSrc:(p:string)=>p,open:async()=>null,save:async()=>null};
const root=document.querySelector<HTMLElement>('#app')!;setupStudio(root,(s)=>{document.querySelector('#status')!.textContent=s;},bridge as any);
root.addEventListener('click',e=>{const tab=(e.target as HTMLElement).closest<HTMLElement>('[data-tab]')?.dataset.tab;if(tab)root.querySelectorAll<HTMLElement>('[data-panel]').forEach(p=>p.hidden=p.dataset.panel!==tab);});
root.querySelector<HTMLButtonElement>('[data-tab="studio"]')!.click();
