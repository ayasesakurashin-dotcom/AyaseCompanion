import {setupP3} from './p3.ts';
import {invoke} from '@tauri-apps/api/core';
import {open,save} from '@tauri-apps/plugin-dialog';
import type {Json,Snapshot} from './types.ts';
import {setupP2} from './p2.ts';
import appIcon from '../src-tauri/icons/icon.png';

const esc=(value:unknown)=>String(value??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]!));
const label=(v:unknown)=>v===true?'正在发生':v===false?'未发生':'状态未知';
const statusNames:Record<string,string>={ready:'等待呈现',preparing:'准备中',starting:'等待开始回执',presenting:'正在呈现',completed:'已完成',cancelled:'已取消',expired:'已过期',failed:'失败',queued:'排队中'};
const id=()=>crypto.randomUUID();
export function setupControl(root:HTMLElement){
  let state:Snapshot|null=null,libraryKey='',pluginsKey='',assetsKey='',nameOwner='',nameDirty=false,bubbleDirty=false;
  root.innerHTML=`<div class="control-shell">
    <header><img class="brand-mark" src="${appIcon}" alt="AyaseCompanion"><div><h1>AyaseCompanion</h1><p>你的桌面伙伴</p></div><span class="version">0.3.0 <b>P3 · 实机验收候选</b></span></header>
    <div class="notice">P3 候选加入屏幕观察、公开资料搜索与默认关怀。新服务与行为需单独配置；尚未发布。</div>
    <nav aria-label="控制台页面"><button class="selected" data-tab="home">伙伴</button><button data-tab="play">播放验证</button><button data-tab="plugins">行为插件</button><button data-tab="diagnostics">运行记录</button></nav>
    <div id="message" role="status" aria-live="polite"></div>
    <section data-panel="home">
      <div class="section-title"><div><h2>伙伴与素材包</h2><p>导入包后，再选择创建伙伴或更新现有伙伴。</p></div><button class="primary" data-action="import">导入 .ayase / .ayplug</button></div>
      <div class="cards"><article><span class="eyebrow">当前伙伴</span><h3 id="active-name">尚未激活</h3><p id="active-detail"></p><div class="actions"><button data-action="input">展开输入框</button><button data-action="reset">找回桌宠</button><button data-action="rollback">回退角色包</button></div></article><article><span class="eyebrow">环境感知</span><p id="facts"></p><small id="facts-detail"></small></article></div>
      <div class="settings-row"><label>显示大小 <input id="zoom" type="range" min="0.6" max="1.6" step="0.1" value="1"></label><label class="switch"><input id="muted" type="checkbox">静音</label><button data-action="stop">停止当前回复</button></div>
      <div class="settings-row"><label>切换已创建的伙伴 <select id="instances"></select></label><button data-action="select-instance">切换</button></div>
      <div class="settings-row"><label>当前伙伴名称 <input id="instance-name" maxlength="80" placeholder="为这位伙伴取个名字"></label><button data-action="rename">保存名称</button></div>
      <details class="bubble-settings"><summary>气泡外观与阅读</summary><p>气泡跟随角色大小，字体单独调整；读完自动收起。设置保存在本机，后续纳入气泡主题。</p><div class="grid-two"><label>文字动画<select id="bubble-animation"><option value="typewriter">打字机</option><option value="none">立即显示</option></select></label><label>显现速度（字 / 秒）<input id="bubble-speed" type="number" min="10" max="80" value="30"></label><label>字号（像素）<input id="bubble-font" type="number" min="12" max="24" value="15"></label><label>每页最少阅读时间（毫秒）<input id="bubble-read" type="number" min="1000" max="10000" value="2800"></label></div><button data-action="save-bubble" class="primary">保存气泡设置</button><p class="hint">完整显示一页后才开始阅读计时；较长页面会自动延长。播放验证中的阅读时间可以延长，不能缩短此下限。</p></details>
      <div id="library" class="list empty">还没有素材包。先导入你自己的角色包。</div>
      <div class="actions bottom"><button data-action="export">导出当前角色包</button><label>导出包版本 <input id="export-version" value="0.3.0" size="12"></label><button data-action="exit" class="quiet">退出应用</button></div>
      <details><summary>全屏检测范围与排除应用</summary><p>仅检测桌宠所在显示器的前台无标题栏全屏窗口；有标题栏的最大化窗口不算全屏。排除项填进程文件名，以逗号分隔。媒体检测使用 Windows 媒体会话，不支持该接口的播放器不会被识别。</p><label>排除应用 <input id="excluded" placeholder="example.exe, another.exe"></label><button data-action="save-excluded">保存</button></details>
    </section>
    <section data-panel="play" hidden>
      <div class="section-title"><div><h2>独立通道，完整交接</h2><p>这里的操作均向宿主提交任务。可以在运行记录中观察通道回执和队列。</p></div></div>
      <div class="play-form"><label>固定文字<textarea id="reply-text" rows="4">你好，我是离线验证中的桌面伙伴。你可以在我说话时打开全屏应用，观察这条回复是否完整结束。</textarea></label>
      <div class="grid-two"><label>固定音声<select id="audio"><option value="">不播放音声</option></select></label><label>动作<select id="cue"><option value="">保持基础姿态</option></select></label></div>
      <div class="grid-two"><label>每页最少阅读时间（毫秒）<input id="read-ms" type="number" value="2800" min="1000" max="30000"></label><label>演示任务优先级<select id="priority"><option value="background">后台</option><option value="normal" selected>普通</option><option value="important">重要</option></select></label></div>
      <div class="actions"><button class="primary" data-action="combined">组合回复</button><button data-action="text-only">纯文字</button><button data-action="audio-only">纯音声</button><button data-action="animation-only">纯动作</button><button data-action="proactive">加入主动互动队列</button></div>
      <div class="actions"><button data-action="greeting">触发启动问候</button><button data-action="long-text">长文字分页示例</button><button class="danger" data-action="stop">停止当前回复</button></div></div>
      <div class="inline-state" id="play-state"></div><p class="hint">验证非抢占：先提交较长的后台任务，待其开始后再提交重要任务。单纯展开输入框或输入草稿应保持当前播放。纯声音与纯动作测试请先选择对应素材。</p>
    </section>
    <section data-panel="plugins" hidden>
      <div class="section-title"><div><h2>行为插件</h2><p>安装插件与为当前伙伴启用是两步。未知插件记录与资产会保留。</p></div><button data-action="import">导入插件包</button></div>
      <div class="settings-row"><select id="plugin-types"></select><button class="primary" data-action="add-preset">添加一份角色配置</button></div>
      <div id="configured"></div><div id="unknown"></div>
      <article class="simulation"><h3>时间触发验证</h3><p>仅向已启用的日程插件发送一次模拟时间事件，不改变系统时钟，也不修改真实计划的去重记录。</p><div class="settings-row"><input id="sim-time" type="datetime-local"><button data-action="simulate-time">模拟这一时刻</button></div></article>
    </section>
    <section data-panel="diagnostics" hidden>
      <div class="section-title"><div><h2>运行记录</h2><p>任务状态、三个通道以及基础姿态交接。</p></div><button data-action="export-diagnostics">导出诊断</button></div>
      <div id="diagnostic-state" class="inline-state"></div><div class="table-wrap"><table><thead><tr><th>任务</th><th>来源</th><th>优先级</th><th>状态</th><th>通道</th></tr></thead><tbody id="task-rows"></tbody></table></div>
      <details open><summary>最近事件</summary><pre id="event-log"></pre></details><p class="hint" id="data-location"></p>
    </section>
    <dialog id="plugin-editor"><form method="dialog"><div class="section-title"><h2 id="editor-title">插件配置</h2><button value="cancel" aria-label="关闭编辑器">×</button></div><p>修改只保存在当前伙伴的本地覆盖中。</p><p id="editor-base" class="hint"></p><div class="grid-two"><label class="switch"><input id="edit-enabled" type="checkbox">启用这份配置</label><label>优先级<select id="edit-priority"><option value="background">后台</option><option value="normal">普通</option><option value="important">重要</option></select></label></div><div id="schema-form"></div><details><summary>完整配置（保留未知字段）</summary><textarea id="edit-json" rows="12" spellcheck="false"></textarea><button type="button" data-action="apply-json">应用到表单</button></details><p id="editor-error" role="alert"></p><div class="actions"><button type="button" class="primary" data-action="save-preset">保存配置</button><button value="cancel">取消</button></div></form></dialog>
  </div>`;
  const updateP2=setupP2(root);
  const updateP3=setupP3(root);
  const $=<T extends HTMLElement=HTMLElement>(s:string)=>root.querySelector<T>(s)!;
  const value=(s:string)=>$<HTMLInputElement>(s).value;
  const message=(s:string,error=false)=>{const el=$('#message');el.textContent=s;el.className=error?'error':'success';};
  const run=async(fn:()=>Promise<unknown>)=>{try{await fn();}catch(e){message(String(e),true);}};
  let editing:Json|null=null,formSchema:Json={};
  const defaults=(schema:Json):any=>schema.properties?.mode?.enum?.includes('generated-with-fallback')&&schema.properties?.variants?{mode:'fixed',variants:[]}:schema.default??(schema.const!==undefined?schema.const:schema.enum?schema.enum[0]:schema.type==='object'?Object.fromEntries(Object.entries(schema.properties??{}).map(([k,v])=>[k,defaults(v as Json)])):schema.type==='array'?[]:schema.type==='boolean'?false:schema.type==='integer'||schema.type==='number'?schema.minimum??0:'');
  function form(){
    const container=$('#schema-form');container.replaceChildren();if(!editing)return;
    function field(schema:Json,current:any,key:string,onChange:(v:any)=>void,parent:HTMLElement){
      const nullable=!!schema.anyOf?.some((s:Json)=>s.type==='null');
      const alternatives=schema.anyOf?.filter((s:Json)=>s.type!=='null');
      if(alternatives)schema={...schema,...(alternatives.find((s:Json)=>s.type===(Array.isArray(current)?'array':'string'))??alternatives[0])};
      const wrap=document.createElement('div');wrap.className='field';parent.append(wrap);
      const title=document.createElement('label');title.textContent=schema.title??key;wrap.append(title);
      let readCurrent=()=>current;
      if(schema.type==='object'&&schema.properties){
        const obj=current&&typeof current==='object'?structuredClone(current):{};
        const group=document.createElement('fieldset');wrap.append(group);
        const reply=!!schema.properties.mode?.enum?.includes('generated-with-fallback')&&!!schema.properties.variants;
        for(const[k,s]of Object.entries(schema.properties)){
          if(reply&&((obj.mode==='state-specific'&&!['mode','stateResponses'].includes(k))||(obj.mode!=='state-specific'&&(k==='stateResponses'||(k==='variants'&&obj.mode!=='fixed')||(k==='fallback'&&obj.mode!=='generated-with-fallback')||(['intent','voice'].includes(k)&&obj.mode==='fixed')||(['cue','animationAsset'].includes(k)&&obj.mode==='fixed')))))continue;
          field(s as Json,obj[k]??defaults(s as Json),k,v=>{obj[k]=v;
            if(reply&&k==='mode'){
              for(const name of ['variants','fallback'])if(Array.isArray(obj[name])&&!obj[name].length)delete obj[name];
              if(!obj.intent)delete obj.intent;
              if(v==='state-specific')obj.stateResponses??=[];
              else if(v==='fixed')obj.variants??=[];
              else{obj.intent??='';if(v==='generated-with-fallback')obj.fallback??=[];}
            }
            onChange(obj);if(reply&&k==='mode')form();},group);
        }
      }else if(schema.type==='array'&&schema.items){
        const arr=Array.isArray(current)?structuredClone(current):[];readCurrent=()=>arr;const list=document.createElement('div');wrap.append(list);
        arr.forEach((entry,index)=>{const item=document.createElement('div');item.className='array-item';list.append(item);field(schema.items,entry,`条目 ${index+1}`,v=>{arr[index]=v;onChange(arr);},item);
          const remove=document.createElement('button');remove.type='button';remove.textContent='删除此项';item.append(remove);remove.onclick=()=>{arr.splice(index,1);onChange(arr);form();};});
        const add=document.createElement('button');add.type='button';add.textContent='添加条目';add.disabled=arr.length>=schema.maxItems;wrap.append(add);add.onclick=()=>{const item=defaults(schema.items);if(item&&typeof item==='object'&&!Array.isArray(item)&&schema.items.properties?.id)item.id=id();arr.push(item);onChange(arr);form();};
      }else if(schema.enum){const select=document.createElement('select');schema.enum.forEach((v:any)=>{const o=document.createElement('option');o.value=String(v);o.textContent=v===null?'通用（所有状态）':v==='state-specific'?'state-specific · 按基础状态选择':String(v);select.append(o);});select.value=String(current);select.onchange=()=>onChange(schema.enum.find((v:any)=>String(v)===select.value));wrap.append(select);
      }else if(schema.type==='boolean'){const input=document.createElement('input');input.type='checkbox';input.checked=Boolean(current);input.onchange=()=>onChange(input.checked);wrap.append(input);
      }else if(schema.type==='object'||Array.isArray(current)){const input=document.createElement('textarea');input.value=JSON.stringify(current,null,2);input.rows=5;input.onchange=()=>{try{onChange(JSON.parse(input.value));input.setCustomValidity('');}catch{input.setCustomValidity('请输入有效 JSON');}};wrap.append(input);
      }else{const input=document.createElement(schema.type==='string'&&(key==='text'||key==='intent'||schema.maxLength>=2000||['audioAsset','textAsset','cue'].includes(key))?'textarea':'input') as HTMLInputElement|HTMLTextAreaElement;
        if(input instanceof HTMLInputElement){input.type=schema.type==='number'||schema.type==='integer'?'number':schema.pattern?.includes('[0-5]')?'time':'text';if(schema.minimum!==undefined)input.min=String(schema.minimum);if(schema.maximum!==undefined)input.max=String(schema.maximum);}
        input.value=String(current??'');input.disabled=schema.const!==undefined;
        readCurrent=()=>input.value;
        input.oninput=()=>onChange(nullable&&input.value===''?null:schema.type==='number'||schema.type==='integer'?Number(input.value):input.value);wrap.append(input);
        if(key==='text'&&alternatives?.some((s:Json)=>s.type==='array')){const multi=document.createElement('button');multi.type='button';multi.textContent='改为多段文字';wrap.append(multi);multi.onclick=()=>{onChange(input.value?[input.value]:[]);form();};}
      }
      if(key==='animationAsset'&&state?.model){const picker=document.createElement('select');picker.setAttribute('aria-label','添加当前模型动画');picker.innerHTML='<option value="">选择动画片段添加到末尾…</option>'+Object.entries(state.model.clips).map(([id,clip])=>`<option value="${esc(id)}">${esc(id)} · ${esc(clip.asset)}</option>`).join('');picker.onchange=()=>{if(!picker.value)return;const value=readCurrent();onChange(Array.isArray(value)?[...value,picker.value]:[String(value??'').trim(),picker.value].filter(Boolean).join('\n'));form();};wrap.append(picker);}
      if(schema.description){const hint=document.createElement('small');hint.textContent=schema.description;wrap.append(hint);}
    }
    field(formSchema,editing.config,'参数',v=>{editing!.config=v;$<HTMLTextAreaElement>('#edit-json').value=JSON.stringify(editing!.config,null,2);},container);
    $<HTMLTextAreaElement>('#edit-json').value=JSON.stringify(editing.config,null,2);
  }
  function edit(c:Json){editing=structuredClone(c);formSchema=state!.plugins.find(p=>p.id===c.pluginId)!.schema;$<HTMLInputElement>('#edit-enabled').checked=c.enabled;$<HTMLSelectElement>('#edit-priority').value=c.priority;$('#editor-title').textContent=state!.plugins.find(p=>p.id===c.pluginId)!.name;$('#editor-error').textContent='';form();$<HTMLDialogElement>('#plugin-editor').showModal();}
  function variant(kind:string){const v:Json={id:id(),readMs:Number(value('#read-ms'))};if(!['audio-only','animation-only'].includes(kind)&&value('#reply-text'))v.text=value('#reply-text');if(!['text-only','animation-only'].includes(kind)&&value('#audio'))v.audioAsset=value('#audio');if(!['text-only','audio-only'].includes(kind)&&value('#cue'))v.cue=value('#cue');return v;}
  root.addEventListener('click',event=>{
    const target=(event.target as HTMLElement).closest<HTMLButtonElement>('button');if(!target)return;
    const tab=target.dataset.tab;if(tab){root.querySelectorAll<HTMLElement>('[data-panel]').forEach(p=>p.hidden=p.dataset.panel!==tab);root.querySelectorAll('[data-tab]').forEach(b=>b.classList.toggle('selected',(b as HTMLElement).dataset.tab===tab));return;}
    const action=target.dataset.action;if(!action)return;
    void run(async()=>{switch(action){
      case 'import':{const path=await open({multiple:false,filters:[{name:'Ayase 包',extensions:['ayase','ayplug','zip']}]});if(typeof path!=='string')return;message('正在检查包、资源和完整性…');const report=await invoke<Json>('import_package',{path});message(`已导入 ${report.displayName}。${report.kind==='character'?'请在素材列表选择创建伙伴或更新。':'请点击安装插件，然后添加角色配置。'}${report.warnings.length?' 提示：'+report.warnings.join('；'):''}`);break;}
      case 'new':case 'update':await invoke('activate_package',{hash:target.dataset.hash,mode:action});message(action==='new'?'已创建并激活新伙伴。':'已更新现有伙伴，旧角色包仍可回退。');break;
      case 'install':await invoke('install_plugin',{hash:target.dataset.hash});message('插件已安装；为当前伙伴添加配置后才会启用。');break;
      case 'uninstall':await invoke('uninstall_plugin',{pluginId:target.dataset.plugin});message('插件已卸载，角色配置与资产保留。');break;
      case 'rollback':await invoke('rollback_package');message('已回退角色包，实例身份保留。');break;
      case 'select-instance':await invoke('select_instance',{id:value('#instances')});break;
      case 'rename':await invoke('user_control',{action:'rename',value:{instanceId:nameOwner,name:value('#instance-name')}});nameDirty=false;message('伙伴名称已保存，身份与配置保持不变。');break;
      case 'save-bubble':await invoke('user_control',{action:'bubble',value:{animation:value('#bubble-animation'),charactersPerSecond:Number(value('#bubble-speed')),fontSize:Number(value('#bubble-font')),readMs:Number(value('#bubble-read'))}});bubbleDirty=false;message('气泡设置已保存。');break;
      case 'input':await invoke('show_window',{label:'input'});break;
      case 'reset':await invoke('reset_position');break;
      case 'stop':await invoke('user_control',{action:'stop',value:null});break;
      case 'exit':await invoke('exit_app');break;
      case 'save-excluded':await invoke('user_control',{action:'excluded',value:value('#excluded').split(',').map(s=>s.trim()).filter(Boolean)});message('全屏排除列表已保存。');break;
      case 'export':{const path=await save({defaultPath:`${state?.package?.id??'character'}-${value('#export-version')}.ayase`,filters:[{name:'Ayase 角色包',extensions:['ayase']}]});if(!path)return;await invoke('export_package',{path,version:value('#export-version')});message('已导出有效配置与原始资产；可重新导入检查。');break;}
      case 'export-diagnostics':{const path=await save({defaultPath:`ayase-diagnostics-${Date.now()}.json`,filters:[{name:'JSON',extensions:['json']}]});if(!path)return;await invoke('export_diagnostics',{path});message('诊断已保存。');break;}
      case 'combined':case 'text-only':case 'audio-only':case 'animation-only':case 'proactive':await invoke('manual_request',{variant:variant(action),priority:value('#priority'),proactive:action==='proactive'});message('任务已加入队列。');break;
      case 'greeting':await invoke('user_control',{action:'greeting',value:null});message('已发送问候事件。');break;
      case 'long-text':$<HTMLTextAreaElement>('#reply-text').value='这是第一段文字，用来检查中文分页和阅读时间。页数会随模型大小和字号变化，切页不会把这条回复拆成多个任务。\n这是第二段文字。当新的重要提醒到达时，它应该等待当前回复；动作提前完成后恢复之前的基础姿态，文字和音声继续。\n这是第三段文字。最后一页完整显示并完成阅读停留后，气泡自动收起。只有本次全部通道都结束，接下来的任务才可以开始。';break;
      case 'add-preset':{const p=state!.plugins.find(p=>p.id===value('#plugin-types'));if(!p)throw Error('请选择已安装插件');edit({presetId:id(),pluginId:p.id,configVersion:p.configVersion,enabled:true,priority:p.priority??'normal',config:structuredClone(p.defaults)});break;}
      case 'edit':{const c=state!.configured.find(c=>c.owner===target.dataset.owner)!;edit({presetId:c.owner,pluginId:c.pluginId,configVersion:'1.0',enabled:c.enabled,priority:c.priority,config:c.config});break;}
      case 'toggle':{const c=state!.configured.find(c=>c.owner===target.dataset.owner)!;await invoke('save_preset',{preset:{presetId:c.owner,pluginId:c.pluginId,configVersion:'1.0',enabled:!c.enabled,priority:c.priority,config:c.config}});break;}
      case 'apply-json':try{editing!.config=JSON.parse(value('#edit-json'));form();$('#editor-error').textContent='';}catch(e){$('#editor-error').textContent=String(e);}break;
      case 'save-preset':try{editing!.enabled=$<HTMLInputElement>('#edit-enabled').checked;editing!.priority=value('#edit-priority');await invoke('save_preset',{preset:editing});$<HTMLDialogElement>('#plugin-editor').close();message('配置已保存，仅影响当前伙伴。');}catch(e){$('#editor-error').textContent=String(e);}break;
      case 'simulate-time':{const time=new Date(value('#sim-time'));if(!Number.isFinite(+time))throw Error('请选择有效的模拟日期时间');await invoke('user_control',{action:'simulate-time',value:time.toISOString()});message('已发送一次模拟时间事件；可在运行记录查看结果。');break;}
    }});
  });
  $('#zoom').addEventListener('change',()=>void run(()=>invoke('user_control',{action:'zoom',value:Number(value('#zoom'))})));
  $('#muted').addEventListener('change',()=>void run(()=>invoke('user_control',{action:'mute',value:$<HTMLInputElement>('#muted').checked})));
  root.querySelectorAll('.bubble-settings input,.bubble-settings select').forEach(input=>input.addEventListener('input',()=>{bubbleDirty=true;}));
  $('#instance-name').addEventListener('input',()=>{nameDirty=true;});
  const date=new Date();date.setHours(23,0,0,0);$<HTMLInputElement>('#sim-time').value=new Date(+date-date.getTimezoneOffset()*60_000).toISOString().slice(0,16);
  return (s:Snapshot)=>{
    updateP2(s);updateP3(s);
    state=s;$('#editor-base').textContent=`当前基础状态：${s.scheduler.desiredBase}。按状态回复优先使用专用项，没有匹配项时跳过。`;$('#active-name').textContent=s.instance?.name??'尚未激活';$('#active-detail').textContent=s.package?`${s.package.id} · ${s.package.version}`:'导入角色包后，选择“创建新伙伴”。';
    if(nameOwner!==(s.instance?.id??''))nameDirty=false;
    if(!nameDirty)$<HTMLInputElement>('#instance-name').value=s.instance?.name??'';
    nameOwner=s.instance?.id??'';
    $<HTMLButtonElement>('[data-action="rename"]').disabled=!nameOwner;
    if(!bubbleDirty&&s.bubble){$<HTMLSelectElement>('#bubble-animation').value=s.bubble.animation;$<HTMLInputElement>('#bubble-speed').value=String(s.bubble.charactersPerSecond);$<HTMLInputElement>('#bubble-font').value=String(s.bubble.fontSize);$<HTMLInputElement>('#bubble-read').value=String(s.bubble.readMs);}
    $('#facts').textContent=`全屏：${label(s.facts.fullscreen)}　媒体：${label(s.facts.media)}`;$('#facts-detail').textContent=`${s.facts.fullscreenStatus}；${s.facts.mediaStatus}${s.facts.mediaSources.length?' · '+s.facts.mediaSources.join('、'):''}`;
    $<HTMLInputElement>('#muted').checked=s.scheduler.muted;if(document.activeElement!==$('#zoom'))$<HTMLInputElement>('#zoom').value=String(s.zoom);
    if(document.activeElement!==$('#excluded'))$<HTMLInputElement>('#excluded').value=s.excluded.join(', ');
    const summary=`当前：${s.scheduler.active?statusNames[s.scheduler.active.status]??s.scheduler.active.status:s.scheduler.baseTransition?'姿态过渡':'空闲'}　基础姿态：${s.scheduler.appliedBase} → ${s.scheduler.desiredBase}　${s.scheduler.suppressed?'新主动互动已抑制':'允许主动互动'}`;
    $('#play-state').textContent=summary;$('#diagnostic-state').textContent=summary;
    const lk=JSON.stringify([s.library,s.instances,s.instance?.id]);if(lk!==libraryKey){libraryKey=lk;$('#library').classList.toggle('empty',!s.library.length);
      $('#library').innerHTML=s.library.map(p=>`<div class="list-row"><div><strong>${esc(p.displayName)}</strong><small>${esc(p.kind==='character'?'角色包':'插件包')} · ${esc(p.version)} · ${p.fileCount} 个文件${p.warnings.length?' · 有资源提示':''}</small></div><div class="actions">${p.kind==='character'?`<button data-action="new" data-hash="${p.hash}">创建新伙伴</button><button data-action="update" data-hash="${p.hash}">更新当前伙伴</button>`:`<button data-action="install" data-hash="${p.hash}">安装插件</button>`}</div></div>`).join('')||'还没有素材包。先导入你自己的角色包。';
      $('#instances').innerHTML=s.instances.map(i=>`<option value="${i.id}" ${i.id===s.instance?.id?'selected':''}>${esc(i.name)} · ${i.id.slice(0,8)}</option>`).join('');
    }
    const pk=JSON.stringify([s.plugins,s.configured,s.unknownPresets]);if(pk!==pluginsKey){pluginsKey=pk;$('#plugin-types').innerHTML=s.plugins.map(p=>`<option value="${esc(p.id)}">${esc(p.name)}</option>`).join('');
      $('#configured').innerHTML=s.configured.map(c=>`<article class="plugin-row"><div><h3>${esc(c.name)}</h3><small>${esc(c.status)} · ${esc(c.priority)} · ${esc(c.owner)}</small></div><div class="actions"><button data-action="edit" data-owner="${esc(c.owner)}">编辑参数</button><button data-action="toggle" data-owner="${esc(c.owner)}">${c.enabled?'关闭':'启用'}</button>${c.pluginId.startsWith('ayase.')?'':`<button data-action="uninstall" data-plugin="${esc(c.pluginId)}">卸载 · 保留配置</button>`}</div></article>`).join('')||'<p class="empty">当前伙伴还没有已安装插件的配置。</p>';
      $('#unknown').innerHTML=s.unknownPresets.map(p=>`<article><h3>保留的未安装插件</h3><p>${esc(p.pluginId)} · ${esc(p.presetId)}</p><small>原始配置和资源保留，当前不执行。</small></article>`).join('');
    }
    const ak=s.package?.hash??'';if(ak!==assetsKey){assetsKey=ak;const assets=s.manifest?.assets??{};
      $('#audio').innerHTML='<option value="">不播放音声</option>'+Object.entries(assets).filter(([,a])=>(a as Json).kind==='audio').map(([k])=>`<option value="${esc(k)}">${esc(k)}</option>`).join('');
      $('#cue').innerHTML='<option value="">保持基础姿态</option>'+Object.keys(s.model?.semanticCues??{}).map(k=>`<option value="${esc(k)}">语义动作 · ${esc(k)}</option>`).join('')+Object.keys(s.model?.clips??{}).map(k=>`<option value="clip:${esc(k)}">片段 · ${esc(k)}</option>`).join('');
    }
    $('#task-rows').innerHTML=[...s.scheduler.tasks].reverse().slice(0,25).map(t=>`<tr><td>#${t.id}</td><td>${esc(t.owner)}</td><td>${esc(t.priority)}</td><td>${esc(statusNames[t.status]??t.status)}</td><td>${esc(Object.entries(t.channels).map(([c,v])=>`${c}: ${v}`).join(' / '))}</td></tr>`).join('');
    $('#event-log').textContent=[...s.scheduler.log].reverse().slice(0,40).map(e=>`${(e.atMs/1000).toFixed(2)}s  ${e.event}  ${JSON.stringify(e.detail)}`).join('\n');$('#data-location').textContent=`独立数据目录：${s.dataDirectory}`;
  };
}
