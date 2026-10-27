import {invoke} from '@tauri-apps/api/core';
import {getCurrentWindow} from '@tauri-apps/api/window';
import type {Snapshot} from './types.ts';
export function setupInput(root:HTMLElement){
  root.innerHTML='<div class="input-shell"><form><textarea rows="2" placeholder="输入固定文本进行离线播放验证…" aria-label="固定回复文本"></textarea><div class="input-footer"><span role="status">离线文字测试 · Enter 发送，Shift+Enter 换行</span><button type="button" data-hide>收起</button><button class="primary">发送</button></div></form></div>';
  const input=root.querySelector('textarea')!,status=root.querySelector('span')!;let composing=false,instance='',saveTimer=0;
  const saveDraft=(owner=instance,text=input.value)=>owner?invoke('user_control',{action:'draft',value:{instanceId:owner,text}}):Promise.resolve();
  input.addEventListener('compositionstart',()=>{composing=true;});input.addEventListener('compositionend',()=>{composing=false;});
  input.addEventListener('input',()=>{clearTimeout(saveTimer);const owner=instance,text=input.value;saveTimer=window.setTimeout(()=>void saveDraft(owner,text).catch(e=>{status.textContent=String(e);}),400);});
  input.addEventListener('keydown',event=>{if(event.key==='Enter'&&!event.shiftKey&&!event.isComposing&&!composing&&event.keyCode!==229){event.preventDefault();root.querySelector('form')!.requestSubmit();}});
  root.querySelector('form')!.addEventListener('submit',event=>{event.preventDefault();if(composing||!input.value.trim())return;const text=input.value,owner=instance;clearTimeout(saveTimer);
    void invoke('manual_request',{instanceId:owner,variant:{id:crypto.randomUUID(),text},priority:'normal',proactive:false}).then(async()=>{if(instance===owner&&input.value===text){input.value='';await saveDraft(owner,'');}status.textContent='已排队，等待当前整条回复完成。';}).catch(e=>{status.textContent=String(e);});
  });
  root.querySelector('[data-hide]')!.addEventListener('click',()=>{void saveDraft().then(()=>getCurrentWindow().hide());});
  return(s:Snapshot)=>{if(instance!==s.instance?.id){if(instance)void saveDraft(instance,input.value).catch(e=>{status.textContent=String(e);});instance=s.instance?.id??'';clearTimeout(saveTimer);input.value=s.instance?.draft??'';}};
}
