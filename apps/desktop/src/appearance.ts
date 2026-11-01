import {invoke} from '@tauri-apps/api/core';
import {renderSchemaForm} from '../../../packages/editor/schema-form.ts';
import themeSchema from '../../../contracts/theme.schema.json';
import {themeLabels} from './studio.ts';
import type {Json,Snapshot} from './types.ts';
export function setupAppearance(root:HTMLElement,message:(s:string,error?:boolean)=>void){
 const button=document.createElement('button');button.dataset.tab='appearance';button.textContent='主题与气泡';root.querySelector('nav')!.append(button);
 const panel=document.createElement('section');panel.dataset.panel='appearance';panel.hidden=true;panel.className='appearance-settings';panel.innerHTML='<div class="section-title"><div><h2>主题、气泡与输入背景</h2><p>全局外观由你选择。导入或切换角色不会自动采用其推荐主题。</p></div></div><article><div class="actions"><button data-appearance="save" class="primary">保存全局外观</button><button data-appearance="recommended">采用当前角色推荐主题</button><button data-appearance="reset">恢复内置默认</button><button data-appearance="reload">放弃未保存编辑</button></div><p class="hint">偏移以当前气泡默认位置为起点，正数向右/向下，使用 Windows 逻辑像素。未指定最大宽高时保持原有随角色变化的限制；指定后仍受屏幕可显示范围约束。短回复按内容收缩。输入背景透明不会降低文字、按钮透明度。</p><div data-appearance-form></div><p class="hint" data-appearance-status></p></article>';
 root.querySelector('.control-shell')!.append(panel);let value:Json|null=null,dirty=false,key='',last:Snapshot|null=null;
 const draw=()=>{if(!value)return;const schema=structuredClone(themeSchema) as Json;for(const p of ['assets','integrity','themeId','version','format','formatVersion'])delete schema.properties[p];renderSchemaForm(panel.querySelector('[data-appearance-form]')!,schema,value,v=>{value=v;dirty=true;},{labels:themeLabels});};
 panel.addEventListener('click',e=>{const a=(e.target as HTMLElement).closest<HTMLButtonElement>('button')?.dataset.appearance;if(!a)return;void(async()=>{try{if(a==='reload'){dirty=false;value=structuredClone(last?.appearance?.settings??{});draw();return;}await invoke('appearance_settings',{action:a==='recommended'?'use-recommended':a,value:a==='save'?value:null});dirty=false;key='';message('全局外观已保存，输入窗口文字和按钮保持清晰。');}catch(e){message(String(e),true);}})();});
 return(s:Snapshot)=>{last=s;const next=JSON.stringify(s.appearance?.settings);if(!dirty&&next!==key&&s.appearance){key=next;value=structuredClone(s.appearance.settings);draw();}panel.querySelector('[data-appearance-status]')!.textContent=dirty?'有未保存的外观编辑。':'已保存的外观会在重启后保留。';};
}
