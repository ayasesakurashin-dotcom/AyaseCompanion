import {renderAssetLibrary,assetCategories} from './studio-assets.ts';
import {invoke,convertFileSrc} from '@tauri-apps/api/core';
import {open,save} from '@tauri-apps/plugin-dialog';
import {renderSchemaForm} from '../../../packages/editor/schema-form.ts';
import {defaultAppearance} from '../../../packages/presentation/appearance.ts';
import {defaultBubblePreferences} from '../../../packages/presentation/bubble.ts';
import {setupPet} from './pet.ts';
import type {Json,Snapshot} from './types.ts';

export const themeLabels:Record<string,string>={displayName:'主题名称',foreground:'文字颜色（#RRGGBB）',background:'背景颜色（#RRGGBB）',borderColor:'边框颜色（#RRGGBB）',fillOpacity:'气泡背景不透明度（%）',borderRadius:'圆角（像素）',paddingTop:'上内边距（像素）',paddingRight:'右内边距（像素）',paddingBottom:'下内边距（像素）',paddingLeft:'左内边距（像素）',tailSize:'尾巴大小（像素）',tailX:'尾巴横向位置（%）',fontSize:'字号（像素）',fontFamily:'字体',animation:'文字显示方式',charactersPerSecond:'显现速度（字/秒）',readMs:'每页最少阅读时间（毫秒）',offsetX:'相对角色左右偏移（像素，正数向右）',offsetY:'相对角色上下偏移（像素，正数向下）',maxWidth:'最大宽度（像素，关闭为自动）',maxHeight:'最大高度（像素，关闭沿用角色高度限制）',inputOpacity:'独立输入窗口背景不透明度（%）',backgroundAsset:'九宫格图片素材编号（关闭为纯色）',sliceTop:'九宫格上切片（原图像素）',sliceRight:'九宫格右切片（原图像素）',sliceBottom:'九宫格下切片（原图像素）',sliceLeft:'九宫格左切片（原图像素）'};
export const themeDefaults=()=>({format:'ayase.theme',formatVersion:'1.0',displayName:'我的气泡主题',...defaultAppearance,...defaultBubblePreferences,assets:{}});
const esc=(s:unknown)=>String(s??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]!));
const uid=()=>crypto.randomUUID();
export function setupStudio(root:HTMLElement,message:(s:string,error?:boolean)=>void,bridge={invoke,convertFileSrc,open,save}){
 const {invoke,convertFileSrc,open,save}=bridge;
 const nav=root.querySelector('nav')!,button=document.createElement('button');button.dataset.tab='studio';button.textContent='制作工作台';nav.append(button);
 const panel=document.createElement('section');panel.dataset.panel='studio';panel.hidden=true;panel.className='studio';panel.innerHTML=`
 <div class="section-title"><div><h2>制作工作台</h2><p>草稿与当前伙伴独立保存。先保存编辑，再预览和导出；预览默认使用模拟服务。</p></div></div>
 <div class="actions"><button data-studio="new-character" class="primary">新建角色</button><button data-studio="new-plugin">新建每日提醒插件</button><button data-studio="new-theme">新建气泡主题</button><button data-studio="open-package">从包制作副本</button><button data-studio="from-current">从当前伙伴制作副本</button><button data-studio="open-workspace">打开项目文件</button></div>
 <div class="settings-row"><label>制作项目<select data-projects></select></label><button data-studio="select">打开所选项目</button><button data-studio="save" class="primary">保存编辑</button><button data-studio="reload">放弃未保存编辑</button><button data-studio="save-workspace">项目另存到新文件夹</button></div>
 <p data-project-status class="hint"></p><div data-editor></div>
 <div data-preview-dock></div><article class="studio-preview-settings" data-preview-card><h3>独立制作预览</h3><p>预览使用与桌宠相同的调度、资源解析和呈现组件。不会写入伙伴聊天、记忆或插件运行记录。</p>
 <div class="studio-preview-controls"><div data-clip-controls hidden><label>动画片段<select data-clip aria-label="选择预览片段"></select></label><button data-studio="preview-clip" class="primary">预览所选片段</button></div><label>预览大小<select data-scale><option value="0.6">60%</option><option value="1" selected>100%</option><option value="1.6">160%</option></select></label><button data-studio="stop">停止制作预览</button></div>
 <p data-preview-state class="hint" role="status"></p><div class="studio-stage" data-preview-surface></div>
 <details class="studio-preview-options"><summary>文字、插件与模拟条件</summary>
 <div class="grid-two"><label>模拟本地日期时间<input data-virtual-time type="datetime-local"></label><label>模拟基础状态<select data-preview-base><option>idle</option><option>music</option><option>quiet</option></select></label><label>插件预设<select data-preview-preset></select></label><label>事件<select data-event><option value="clock.tick">模拟时钟事件</option><option value="pointer.double">双击</option><option value="pointer.single">单击</option><option value="app.ready">应用启动</option><option value="environment.media">媒体环境更新</option><option value="environment.fullscreen">全屏环境更新</option></select></label><label>模拟累计活跃时长（秒）<input data-elapsed type="number" min="0" value="4000"></label></div>
 <label class="switch"><input data-environment type="checkbox" checked>模拟环境事件正在发生</label>
 <div class="grid-two"><label>模拟天气（可选）<select data-weather><option value="">未提供</option><option value="rain">降雨</option><option value="hot">高温</option><option value="cold">低温</option></select></label><label>点击区域编号（可选）<input data-area></label></div>
 <div class="grid-two"><label>模拟日历事实<select data-calendar><option value="">无</option><option value="public">全国公众假日</option><option value="workday">已核对工作日</option><option value="traditional">传统节日</option><option value="solar-term">节气</option></select></label><label class="switch"><input data-special type="checkbox">模拟一项当天特殊日期</label></div>
 <label>预览文字<textarea data-preview-text rows="3">你好，这是我的桌面伙伴。</textarea></label>
 <div class="actions"><button data-studio="sample-short">填入中文短句</button><button data-studio="sample-long">填入长文分页示例</button><button data-studio="preview-text">预览文字与语义动作</button><button data-studio="preview-preset">模拟触发所选插件</button></div>
 <details><summary>真实服务预览（需主动开启，可能计费）</summary><label class="switch"><input data-real type="checkbox">允许本次生成预览使用程序中已保存的聊天服务</label><label class="switch"><input data-real-voice type="checkbox">同时合成预览语音</label><p>仅显式点击插件预览时生效。使用项目人格与当前伙伴的音色/语言设置；不访问记忆、屏幕、搜索或伙伴历史。</p></details>

 </details></article>
 <article><h3>检查与分发</h3><div class="actions"><button data-studio="validate">检查完整包</button><button data-studio="export" class="primary">导出新版本包</button><button data-studio="import-plugin">导入插件到制作工作台</button></div><pre data-report>导出前会使用与正式导入相同的完整校验。</pre></article>`;
 root.querySelector('.control-shell')!.append(panel);
 const $=<T extends HTMLElement=HTMLElement>(s:string)=>panel.querySelector<T>(s)!;
 const val=(s:string)=>$<HTMLInputElement>(s).value;
 const previewCard=$('[data-preview-card]'),previewDock=$('[data-preview-dock]');
 const groupStates=new Map<string,Map<string,boolean>>(),formStates=new Map<string,Map<string,boolean>>(),assetCache=new Map<string,string>();let formIndex=0,assetFilter='',selectedClip='',assetCacheScope='';
 let snapshot:Json|null=null,doc:Json|null=null,project='',dirty=false,asset='',preset='',tab='identity',running=false,disposed=false,projectKey='';
 const command=<T=any>(action:string,payload:Json={})=>invoke<T>('studio_command',{action,payload:{projectId:project,...payload}});
 const pet=setupPet($('[data-preview-surface]'),{assetUrl:convertFileSrc,drag:async()=>{},invoke:(async(cmd:string,args?:Json)=>{
  if(cmd==='presentation_ack'||cmd==='base_transition_ack')return invoke('studio_ack',{projectId:String(args!.instanceId??'').replace(/^studio:/,'').split(':')[0],revision:Number(String(args!.instanceId??'').split(':')[2]??-1),id:args!.id,generation:args!.generation,channel:args!.channel??'',event:cmd==='base_transition_ack'?`base-${args!.event}`:args!.event});
  if(cmd==='user_control')return command('stop');return true;
 }) as typeof invoke});
 function note(){const c=snapshot?.current;$('[data-project-status]').textContent=c?`${doc?.manifest.displayName??'未命名'} · ${doc?.kind} · 修订 ${doc?.revision}${dirty?' · 有未保存编辑':' · 已保存'}。内部项目自动保留，可另存 project.json 和素材。`: '请选择新建角色、插件或主题。';}
 function changed(){dirty=true;note();}
 async function refresh(force=false){const s=await invoke<Json>('studio_snapshot');snapshot=s;const c=s.current;const nextCacheScope=c?`${c.id}:${c.document.revision}`:'';if(nextCacheScope!==assetCacheScope){assetCache.clear();assetCacheScope=nextCacheScope;}const key=JSON.stringify(s.projects);if(key!==projectKey){projectKey=key;$<HTMLSelectElement>('[data-projects]').innerHTML=s.projects.map((p:Json)=>`<option value="${esc(p.id)}">${esc(p.name)} · ${esc(p.kind)}</option>`).join('');}
  if(c&&(c.id!==project||force)){const switched=c.id!==project;project=c.id;doc=structuredClone(c.document);dirty=false;if(switched){asset='';preset='';assetFilter='';selectedClip='';}render();}else if(!c){doc=null;project='';$('[data-editor]').replaceChildren();}
  if(project)$<HTMLSelectElement>('[data-projects]').value=project;note();
  const p=s.preview;const model=p.model;const appearance=p.theme??themeDefaults();await pet({instance:model?{id:`studio:${p.projectId}`,packageHash:String(p.revision),name:'制作预览'}:null,package:model?{hash:String(p.revision)}:null,model,assetPaths:p.assetPaths,zoom:p.zoom??1,bubble:{...defaultBubblePreferences,...appearance},appearance:{settings:appearance,assetPaths:p.themeAssetPaths??p.assetPaths},scheduler:p.scheduler,petInteractive:false} as Snapshot);
  const active=p.scheduler.active;$('[data-preview-state]').textContent=`${active?`任务 #${active.id} · ${active.status}`:'预览空闲'}${s.notice?' · '+s.notice:''} · ${$<HTMLInputElement>('[data-real]').checked?'真实服务仅在显式生成预览中启用':'模拟服务，无联网调用'}`;
 }
 function bindField(container:HTMLElement,key:string,label:string,obj:Json,type='text'){
  const l=document.createElement('label');l.textContent=label;const input=document.createElement(type==='textarea'?'textarea':'input') as HTMLInputElement|HTMLTextAreaElement;if(input instanceof HTMLTextAreaElement)input.rows=5;else input.type=type;input.value=String(obj[key]??'');input.oninput=()=>{obj[key]=type==='number'?Number(input.value):input.value;changed();};l.append(input);container.append(l);
 }
 function form(container:HTMLElement,schema:Json,obj:any,change:(v:any)=>void){renderSchemaForm(container,schema,obj,v=>{change(v);changed();},{model:doc?.model,labels:themeLabels,collapsible:true,foldState:formState()});}
 function formState(){const scope=`${project}:${tab}:${tab==='presets'?preset:''}:${formIndex++}`;if(!formStates.has(scope))formStates.set(scope,new Map());return formStates.get(scope)!;}
 function editorJson(parent:HTMLElement,label:string,current:any,change:(v:any)=>void){const details=document.createElement('details');details.innerHTML=`<summary>${esc(label)}</summary>`;const input=document.createElement('textarea');input.rows=16;input.value=JSON.stringify(current,null,2);const b=document.createElement('button');b.textContent='应用这段 JSON 到草稿';b.onclick=()=>{try{change(JSON.parse(input.value));changed();render();}catch(e){message(String(e),true);}};details.append(input,b);parent.append(details);}
 function render(){if(!doc||!snapshot)return;formIndex=0;previewDock.after(previewCard);const d=doc,c=snapshot.current,m=d.manifest,editor=$('[data-editor]');const tabs=d.kind==='character'?{identity:'角色与人格',assets:'素材库',model:'动作与映射',presets:'插件配置',theme:'推荐气泡主题'}:d.kind==='plugin'?{identity:'插件与默认配置',rules:'规则与参数',assets:'附带素材'}:{theme:'主题设置',assets:'九宫格素材'};if(!Object.hasOwn(tabs,tab))tab=Object.keys(tabs)[0];
 editor.innerHTML=`<div class="studio-tabs actions">${Object.entries(tabs).map(([k,v])=>`<button data-studio-tab="${k}" class="${tab===k?'primary':''}">${v}</button>`).join('')}</div><article data-edit-content></article>`;const content=$('[data-edit-content]');
 if(tab==='identity'){
  const grid=document.createElement('div');grid.className='grid-two';content.append(grid);bindField(grid,d.kind==='character'?'packageId':'pluginId',d.kind==='character'?'角色包 ID（发布后保持）':'插件 ID（发布后保持）',m);bindField(grid,'version','包版本（如 0.1.0，另存版本时递增）',m);bindField(grid,'displayName','显示名称',m);
  if(d.kind==='character'){m.identity??={};bindField(grid,'characterName','角色称呼',m.identity);bindField(grid,'userAddress','对用户的称呼',m.identity);bindField(content,'persona','固定人格',m.identity,'textarea');}
  else{const h=document.createElement('h3');h.textContent='每日提醒公共参数与默认回复';content.append(h);const f=document.createElement('div');content.append(f);form(f,d.configSchema,m.defaults,v=>m.defaults=v);}
  editorJson(content,'高级清单（保留原有未知字段）',m,v=>d.manifest=v);
 }
 if(tab==='assets'){
  content.innerHTML=`<h3>素材库</h3><p>按分类添加素材；图片图集显示首帧，文字分类展开后显示摘要。已有编号可替换素材，引用保持不变。</p><div class="settings-row"><label>导入或替换编号（单个文件可填写）<input data-asset-id placeholder="例如 voice.morning"></label><button data-studio="add-assets">添加 / 替换文件</button><label>查找素材<input data-asset-search type="search" placeholder="素材编号或文件名" value="${esc(assetFilter)}"></label></div><div data-asset-library></div><div data-asset-detail></div>`;
  const groupKey=project;if(!groupStates.has(groupKey))groupStates.set(groupKey,new Map());
  const drawAssets=()=>{const owner=project;renderAssetLibrary($('[data-asset-library]'),{assets:m.assets??{},paths:c.assetPaths??{},files:c.files??[],model:d.model,selected:asset,filter:assetFilter,scope:`${project}:${d.revision}`,openState:groupStates.get(groupKey)!,cache:assetCache,assetUrl:convertFileSrc,readText:id=>command<string>('text',{projectId:owner,assetId:id})});};
  drawAssets();$<HTMLInputElement>('[data-asset-search]').oninput=e=>{assetFilter=(e.target as HTMLInputElement).value;drawAssets();};
  if(asset&&m.assets?.[asset]){const a=m.assets[asset],detail=$('[data-asset-detail]');detail.innerHTML=`<h3>${esc(asset)}</h3><p>${esc(a.path)}</p><div class="actions"><button data-studio="references">查看引用</button><label>新编号<input data-rename value="${esc(asset)}"></label><button data-studio="rename">重命名并更新引用</button><button data-studio="delete-asset" class="danger">删除未被引用素材</button>${a.kind==='audio'?'<button data-studio="preview-audio">按任务试听</button>':''}${a.kind==='image'&&d.kind==='character'?'<button data-studio="static-idle">以此图创建静态待机角色</button>':''}</div><pre data-references></pre>`;
   const path=c.assetPaths?.[asset];if(a.kind==='image'&&path){const img=document.createElement('img');img.className='asset-preview';img.src=convertFileSrc(path);img.alt=asset;detail.prepend(img);}if(a.kind==='text'){const input=document.createElement('textarea');input.dataset.assetText='';input.rows=6;detail.append(input);const b=document.createElement('button');b.textContent='保存文字素材';b.onclick=()=>void run(async()=>{await saveDraft();await command('save-text',{assetId:asset,text:input.value});await refresh(true);});detail.append(b);void command<string>('text',{assetId:asset}).then(t=>{if(input.isConnected)input.value=t;});}
  }
 }
 if(tab==='model'){
  content.innerHTML='<h3>动作、基础姿态与语义映射</h3><p>单图、网格图集和逐帧矩形共用同一模型。编号可在插件的 animationAsset 中直接引用。</p><div class="actions"><button data-studio="import-clips">导入完整动作清单</button></div><div data-model-preview-host></div><div data-model-form></div>';
  $('[data-model-preview-host]').append(previewCard);
form($('[data-model-form]'),snapshot.contracts.sprite,d.model,v=>d.model=v);editorJson(content,'高级模型 JSON',d.model,v=>d.model=v);
 }
 if(tab==='presets'){
  content.innerHTML=`<h3>角色插件配置</h3><p>可跳过全部插件。固定与生成回复均可设置 idle/music/quiet 条件；未知插件原样保留，不参与预览执行。</p><div class="settings-row"><select data-plugin-type>${snapshot.plugins.map((p:Json)=>`<option value="${esc(p.id)}">${esc(p.name)} · ${esc(p.id)}</option>`).join('')}</select><button data-studio="add-preset">添加插件配置</button><button data-studio="import-plugin">导入独立插件</button><button data-studio="import-presets">合并导入预设清单</button></div><div class="actions">${(m.pluginPresets??[]).map((p:Json)=>`<button data-preset="${esc(p.presetId)}" class="${preset===p.presetId?'primary':''}">${esc(p.presetId)} · ${esc(p.pluginId)}</button>`).join('')}</div><div data-preset-form></div>`;
  const p=m.pluginPresets?.find((p:Json)=>p.presetId===preset);if(p){const f=$('[data-preset-form]'),plugin=snapshot.plugins.find((x:Json)=>x.id===p.pluginId);bindField(f,'presetId','配置编号（同一包内唯一）',p);const options=document.createElement('div');f.append(options);form(options,{type:'object',properties:{enabled:{type:'boolean',title:'启用'},priority:{enum:['background','normal','important'],title:'优先级'}}},p,v=>Object.assign(p,v));if(plugin){const b=document.createElement('button');b.textContent='恢复此插件默认值';b.onclick=()=>{p.config=structuredClone(plugin.defaults);p.configMode='complete';changed();render();};f.append(b);const config=document.createElement('div');f.append(config);form(config,plugin.schema,p.config,v=>{p.config=v;p.configMode='complete';});}else f.append(document.createTextNode('未知插件，保留原配置；导入对应插件后恢复表单。'));editorJson(f,'完整预设 JSON',p,v=>{m.pluginPresets[m.pluginPresets.indexOf(p)]=v;});const remove=document.createElement('button');remove.textContent='从此角色删除这份配置';remove.onclick=()=>{m.pluginPresets=m.pluginPresets.filter((v:Json)=>v!==p);preset='';changed();render();};f.append(remove);}
 }
 if(tab==='theme'){
  if(d.kind==='character'&&d.theme===null){content.innerHTML='<h3>角色推荐主题</h3><p>推荐主题不会自动覆盖用户的全局主题。</p><button data-studio="create-theme">添加内置默认主题作为起点</button>';}
  else {const v=d.kind==='theme'?m:d.theme;content.innerHTML='<h3>气泡主题与输入窗口背景</h3><p>像素采用 Windows 逻辑像素；正偏移向右/向下。短文字不会强行撑到最大宽高。手动最大高度可超过模型，但仍受屏幕限制。透明度只影响背景。</p><div data-theme-form></div>';const filtered=structuredClone(snapshot.contracts.theme);for(const key of ['assets','integrity','themeId','version'])delete filtered.properties[key];form($('[data-theme-form]'),filtered,v,next=>{if(d.kind==='theme')d.manifest=next;else d.theme=next;const theme=d.kind==='theme'?d.manifest:d.theme;const background=theme.backgroundAsset;if(background&&d.manifest.assets?.[background]){theme.assets??={};theme.assets[background]=structuredClone(d.manifest.assets[background]);}});if(d.kind==='character'){const b=document.createElement('button');b.textContent='移除此角色的推荐主题';b.onclick=()=>{d.theme=null;delete m.recommendedTheme;changed();render();};content.append(b);}}
 }
 if(tab==='rules'){content.innerHTML='<h3>声明式规则与参数定义</h3><p>每日提醒模板可直接在“插件与默认配置”中完成。高级规则只允许协议中的事件、条件和动作，不执行脚本。</p><div data-rules-form></div>';form($('[data-rules-form]'),snapshot.contracts.rules,d.rules,v=>d.rules=v);editorJson(content,'公共参数 JSON Schema（协议支持的子集）',d.configSchema,v=>d.configSchema=v);editorJson(content,'高级规则 JSON',d.rules,v=>d.rules=v);}
 $('[data-clip-controls]').hidden=!(d.kind==='character'&&tab==='model');const clips=Object.keys(d.model?.clips??{});$<HTMLSelectElement>('[data-clip]').innerHTML=clips.map(id=>`<option value="${esc(id)}">${esc(id)}</option>`).join('');if(!clips.includes(selectedClip))selectedClip=clips[0]??'';$<HTMLSelectElement>('[data-clip]').value=selectedClip;$<HTMLButtonElement>('[data-studio=preview-clip]').disabled=!selectedClip;
 const old=$<HTMLSelectElement>('[data-preview-preset]').value;$<HTMLSelectElement>('[data-preview-preset]').innerHTML=(m.pluginPresets??[]).map((p:Json)=>`<option value="${esc(p.presetId)}">${esc(p.presetId)}</option>`).join('');if(old)$<HTMLSelectElement>('[data-preview-preset]').value=old;
 }
 async function saveDraft(){if(!doc)throw Error('请先打开项目');if(!dirty)return;await command('save',{revision:doc.revision,document:doc});await refresh(true);}
 const run=async(fn:()=>Promise<void>)=>{if(running)return;running=true;panel.setAttribute('aria-busy','true');try{await fn();}catch(e){message(String(e),true);}finally{running=false;panel.removeAttribute('aria-busy');}};
 async function preview(options:Json){await saveDraft();const event=val('[data-event]'),weather=val('[data-weather]');const r=await command('preview',{base:val('select[data-preview-base]'),zoom:Number(val('[data-scale]')),wall:new Date(val('[data-virtual-time]')).toISOString(),elapsedMs:Number(val('[data-elapsed]'))*1000,area:val('[data-area]'),click:event.endsWith('single')?'single':'double',event,environmentActive:$<HTMLInputElement>('[data-environment]').checked,weather:weather?{city:'模拟地点',temperature:weather==='hot'?40:weather==='cold'?-5:20,code:weather==='rain'?61:0}:null,calendar:val('[data-calendar]')?[{category:val('[data-calendar]'),title:'模拟日期事实'}]:[],special:$<HTMLInputElement>('[data-special]').checked?[{title:'模拟纪念日'}]:[],...options});message(`制作预览任务 #${r.taskId} 已提交。`);await refresh();}
 panel.addEventListener('click',e=>{const b=(e.target as HTMLElement).closest<HTMLButtonElement>('button');if(!b)return;if(b.dataset.studioTab){tab=b.dataset.studioTab;render();return;}if(b.dataset.asset){asset=b.dataset.asset;render();return;}if(b.dataset.preset){preset=b.dataset.preset;render();return;}const a=b.dataset.studio;if(!a)return;void run(async()=>{switch(a){
  case 'new-character':case 'new-plugin':case 'new-theme':if(dirty)await saveDraft();await command('new',{kind:a.slice(4)});tab=a==='new-theme'?'theme':'identity';await refresh(true);break;
  case 'open-package':case 'open-workspace':{if(dirty)await saveDraft();const path=await open({multiple:false,filters:[{name:a==='open-package'?'Ayase 包':'项目文件',extensions:a==='open-package'?['ayase','ayplug','aytheme']:['json']}]});if(typeof path!=='string')return;await command(a,{path});await refresh(true);break;}
  case 'from-current':if(dirty)await saveDraft();await command('from-current');await refresh(true);break;
  case 'select':if(dirty)await saveDraft();await command('select',{projectId:val('[data-projects]')});await refresh(true);break;
  case 'save':await saveDraft();message('制作项目已保存。');break;
  case 'reload':await refresh(true);break;
  case 'save-workspace':{await saveDraft();const path=await save({defaultPath:`${doc!.manifest.displayName??'我的项目'}-制作项目`});if(!path)return;const r=await command('save-workspace',{path});message(`已另存项目：${r.path}`);break;}
  case 'add-assets':{const name=val('[data-asset-id]');const category=assetCategories.find(c=>c.kind===b.dataset.assetKind);await saveDraft();const paths=await open({multiple:true,filters:[category?{name:category.name,extensions:[...category.extensions]}:{name:'图像、音声或文字素材',extensions:['png','webp','mp3','wav','ogg','flac','txt']}]});if(!paths)return;const list=typeof paths==='string'?[paths]:paths;await command('add-assets',{items:list.map(path=>({path,...(list.length===1&&name?{id:name}:{})}))});await refresh(true);break;}
  case 'references':$('[data-references]').textContent=JSON.stringify(await command('references',{assetId:asset}),null,2);break;
  case 'rename':{const to=val('[data-rename]');await saveDraft();await command('rename-asset',{assetId:asset,newId:to});await refresh(true);asset=to;render();break;}
  case 'delete-asset':await saveDraft();await command('delete-asset',{assetId:asset});asset='';await refresh(true);break;
  case 'static-idle':{const a=doc!.manifest.assets[asset];const img=new Image();img.src=convertFileSrc(snapshot!.current.assetPaths[asset]);await img.decode();const width=Math.min(4096,img.naturalWidth),height=Math.min(4096,img.naturalHeight);if(img.naturalWidth>4096||img.naturalHeight>4096)throw Error('静态图长宽不能超过 4096 像素');doc!.model={schemaVersion:'1.0',logicalSize:{width,height},anchors:{feet:{x:.5,y:1},bubble:{x:.5,y:0}},clips:{idle:{asset,layout:{type:'single'},frameDurationMs:1000,direction:'forward'}},baseModes:{idle:{loop:'idle'}},semanticCues:{}};void a;changed();tab='model';render();break;}
  case 'import-clips':{await saveDraft();const path=await open({filters:[{name:'完整模型动作清单',extensions:['json']}]});if(typeof path!=='string')return;await command('import-clips',{path});await refresh(true);break;}
  case 'preview-clip':{const clip=val('[data-clip]');selectedClip=clip;await preview({clip});break;}
  case 'preview-audio':await preview({audioAsset:asset});break;
  case 'add-preset':{const p=snapshot!.plugins.find((p:Json)=>p.id===val('[data-plugin-type]'));if(!p)throw Error('请先导入插件');preset=uid();doc!.manifest.pluginPresets??=[];doc!.manifest.pluginPresets.push({presetId:preset,pluginId:p.id,configVersion:p.configVersion,configMode:'complete',enabled:true,priority:p.priority??'normal',config:structuredClone(p.defaults)});changed();render();break;}
  case 'import-presets':{await saveDraft();const path=await open({filters:[{name:'角色插件预设清单',extensions:['json']}]});if(typeof path!=='string')return;await command('import-presets',{path});await refresh(true);break;}
  case 'import-plugin':{const path=await open({filters:[{name:'规则插件',extensions:['ayplug']}]});if(typeof path!=='string')return;await command('import-plugin',{path});await refresh();render();message('插件已导入制作工作台；当前伙伴未安装或启用此插件。');break;}
  case 'create-theme':doc!.theme=themeDefaults();changed();render();break;
  case 'sample-short':$<HTMLTextAreaElement>('[data-preview-text]').value='下午好！记得稍微休息。';break;
  case 'sample-long':$<HTMLTextAreaElement>('[data-preview-text]').value=('这是中文长文预览，用来检查气泡宽高、字体、行距、分页、阅读停留与角色的相对位置。标点、数字 123 和换行都应完整保留。\n').repeat(8);break;
  case 'preview-text':await preview({text:val('[data-preview-text]')});break;
  case 'preview-preset':await preview({presetId:val('[data-preview-preset]'),realServices:$<HTMLInputElement>('[data-real]').checked,realVoice:$<HTMLInputElement>('[data-real-voice]').checked});break;
  case 'stop':await command('stop');await refresh();break;
  case 'validate':await saveDraft();$('[data-report]').textContent=JSON.stringify(await command('validate'),null,2);message('完整校验通过。');break;
  case 'export':{await saveDraft();const ext=doc!.kind==='character'?'ayase':doc!.kind==='plugin'?'ayplug':'aytheme';const path=await save({defaultPath:`${doc!.manifest.displayName??'我的作品'}-${doc!.manifest.version??'0.1.0'}.${ext}`,filters:[{name:'Ayase 分发包',extensions:[ext]}]});if(!path)return;$('[data-report]').textContent=JSON.stringify(await command('export',{path}),null,2);message('已导出并通过导入校验。草稿元数据、实例和服务配置不会进入分发包。');break;}
 }});});
 $<HTMLSelectElement>('[data-clip]').onchange=()=>{selectedClip=val('[data-clip]');};
 const date=new Date();date.setHours(23,0,0,0);$<HTMLInputElement>('[data-virtual-time]').value=new Date(+date-date.getTimezoneOffset()*60000).toISOString().slice(0,16);
 let wasVisible=false;const poll=async()=>{if(disposed)return;try{if(!panel.hidden){wasVisible=true;if(!running)await refresh();}else if(wasVisible){wasVisible=false;await command('stop');const empty={instance:null,petInteractive:false} as Snapshot;await pet(empty);}}catch(e){$('[data-preview-state]').textContent=String(e);}finally{setTimeout(poll,panel.hidden?600:snapshot?.preview.scheduler.active?90:500);}};void poll();window.addEventListener('beforeunload',()=>{disposed=true;});
 return ()=>{};
}
