import {invoke} from '@tauri-apps/api/core';
import {getCurrentWindow} from '@tauri-apps/api/window';
import type {Snapshot} from './types.ts';
export function setupInput(root:HTMLElement){
  root.innerHTML='<div class="input-shell"><form><select aria-label="输入方式"><option value="chat">与伙伴聊天</option><option value="offline">离线固定文字验证</option></select><textarea rows="2" placeholder="对当前伙伴说点什么…" aria-label="消息文本"></textarea><div class="input-footer"><span role="status">Enter 发送，Shift+Enter 换行</span><button type="button" data-hide>收起</button><button class="primary">发送</button></div></form></div>';
  const input=root.querySelector('textarea')!,status=root.querySelector('span')!;let composing=false,instance='',saveTimer=0,sentId:number|null=null,sentOwner='';
  const saveDraft=(owner=instance,text=input.value)=>owner?invoke('user_control',{action:'draft',value:{instanceId:owner,text}}):Promise.resolve();
  input.addEventListener('compositionstart',()=>{composing=true;});input.addEventListener('compositionend',()=>{composing=false;});
  input.addEventListener('input',()=>{clearTimeout(saveTimer);const owner=instance,text=input.value;saveTimer=window.setTimeout(()=>void saveDraft(owner,text).catch(e=>{status.textContent=String(e);}),400);});
  input.addEventListener('keydown',event=>{if(event.key==='Enter'&&!event.shiftKey&&!event.isComposing&&!composing&&event.keyCode!==229){event.preventDefault();root.querySelector('form')!.requestSubmit();}});
  root.querySelector('form')!.addEventListener('submit',event=>{event.preventDefault();if(composing||!input.value.trim())return;const text=input.value,owner=instance;clearTimeout(saveTimer);
    const offline=root.querySelector('select')!.value==='offline';
    const request=offline?invoke('manual_request',{instanceId:owner,variant:{id:crypto.randomUUID(),text},priority:'normal',proactive:false}):invoke('chat_request',{instanceId:owner,text});
    void request.then(async(id)=>{if(typeof id==='number'){sentId=id;sentOwner=owner;}if(instance===owner&&input.value===text){input.value='';await saveDraft(owner,'');}status.textContent='请求已接受；处理顺序按当前伙伴的发送策略执行。';}).catch(e=>{status.textContent=String(e);});
  });
  root.querySelector('[data-hide]')!.addEventListener('click',()=>{void saveDraft().then(()=>getCurrentWindow().hide());});
  return(s:Snapshot)=>{if(sentId!==null&&sentOwner===s.instance?.id){const task=s.scheduler.tasks.find(t=>t.id===sentId);if(task)status.textContent=task.plan.serviceError?`生成未完整结束，已保留收到的正文：${task.plan.serviceError}`:['failed','expired','cancelled'].includes(task.status)?`回复未完成：${task.plan.serviceError??task.result??'已取消'}`:task.status==='completed'?'回复已完整结束。':task.status==='presenting'?'伙伴正在回复；新消息会按所选策略处理。':'请求正在准备或排队；可以按停止取消。';}if(instance!==s.instance?.id){if(instance)void saveDraft(instance,input.value).catch(e=>{status.textContent=String(e);});sentId=null;sentOwner='';status.textContent='Enter 发送，Shift+Enter 换行';instance=s.instance?.id??'';clearTimeout(saveTimer);input.value=s.instance?.draft??'';}};
}
