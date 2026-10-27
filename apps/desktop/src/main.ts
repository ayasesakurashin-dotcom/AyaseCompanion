import {invoke,isTauri} from '@tauri-apps/api/core';
import {setupPet} from './pet.ts';
import {setupControl} from './control.ts';
import {setupInput} from './input.ts';
import type {Snapshot} from './types.ts';
import './style.css';
const root=document.querySelector<HTMLElement>('#app')!;
const view=new URLSearchParams(location.search).get('view')??'control';document.body.className=view;document.documentElement.className=view;
if(!isTauri()){
  root.innerHTML='<div class="browser-notice"><h1>AyaseCompanion</h1><p>这是桌面应用，请运行提供的 AyaseCompanion 程序。浏览器预览无法访问本机窗口、角色包或 Windows 媒体能力。</p></div>';
}else{
  const update=view==='pet'?setupPet(root):view==='input'?setupInput(root):setupControl(root);
  let stopped=false;
  const poll=async()=>{try{await update(await invoke<Snapshot>('snapshot'));}catch(error){console.error(error);}finally{if(!stopped)setTimeout(poll,view==='pet'?80:500);}};
  void poll();window.addEventListener('beforeunload',()=>{stopped=true;});
}
