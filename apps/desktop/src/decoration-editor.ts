import {decorationDefaults,orderedDecorations,decorationAsset,sourceRect,type Decoration} from '../../../packages/presentation/decorations.ts';
import type {Model} from '../../../packages/presentation/model.ts';

type Options={model:Model;assets:Record<string,{kind:string}>;paths:Record<string,string>;url:(p:string)=>string;changed:()=>void;selected:(id:string)=>void;preview:()=>void;clips:()=>void};
export function renderDecorationEditor(root:HTMLElement,o:Options){
  o.model.decorations??=[];o.model.decorationVersion='1.0';let selected=o.model.decorations[0]?.id??'';
  const open=new Map<string,boolean>();
  const button=(text:string,fn:()=>void)=>{const b=document.createElement('button');b.type='button';b.textContent=text;b.onclick=fn;return b;};
  function changed(redraw=false){o.changed();if(redraw){draw();return;}for(const row of root.querySelectorAll<HTMLElement>('.decoration-row')){const d=o.model.decorations!.find(d=>d.id===row.dataset.layerId);if(!d)continue;row.querySelector('span')!.textContent=d.name||d.id;row.querySelector('small')!.textContent=`${d.enabled?'启用':'关闭'} · ${d.states===null?'全状态':d.states.join(' / ')||'无状态'} · ${d.speed} 倍`;}const d=o.model.decorations!.find(d=>d.id===selected);const title=root.querySelector('.decoration-form h3');if(d&&title)title.textContent=d.name||'装饰参数';}
  function draw(){
    root.querySelectorAll<HTMLDetailsElement>('details[data-fold]').forEach(d=>open.set(d.dataset.fold!,d.open));root.replaceChildren();
    const intro=document.createElement('p');intro.textContent='从上到下表示组内从靠前到靠后。背景始终在角色后，前景始终在角色前；全状态层可以与专用状态层同时显示。';root.append(intro);
    const actions=document.createElement('div');actions.className='actions';actions.append(button('保存并开始整体预览',o.preview),button('创建或编辑动画片段',o.clips));root.append(actions);
    const layout=document.createElement('div');layout.className='decoration-editor';root.append(layout);
    const lists=document.createElement('div'),form=document.createElement('div');form.className='decoration-form';layout.append(lists,form);
    for(const side of ['background','foreground'] as const){
      const group=document.createElement('details');group.dataset.fold=side;group.open=open.get(side)??true;
      const summary=document.createElement('summary');summary.textContent=side==='background'?'背景（角色后方）':'前景（角色前方）';group.append(summary);
      group.append(button(`添加${side==='background'?'背景':'前景'}`,()=>{const d=decorationDefaults(crypto.randomUUID(),side);const asset=Object.keys(o.assets).find(id=>o.assets[id].kind==='image')??'';d.source={type:'image',asset};d.order=Math.max(0,...o.model.decorations!.filter(x=>x.side===side).map(x=>x.order))+1;o.model.decorations!.push(d);selected=d.id;changed(true);}));
      const layers=orderedDecorations(o.model.decorations!).filter(d=>d.side===side).reverse();
      if(!layers.length){const empty=document.createElement('p');empty.textContent='尚无装饰，可保持为空。';group.append(empty);}
      for(const d of layers){
        const row=document.createElement('div');row.className='decoration-row';row.dataset.layerId=d.id;row.dataset.selected=String(selected===d.id);
        const select=button('',()=>{selected=d.id;draw();});select.className='decoration-select';
        const thumb=document.createElement('canvas');thumb.width=80;thumb.height=54;thumb.className='decoration-thumb';select.append(thumb);
        const name=document.createElement('span');name.textContent=d.name||d.id;select.append(name);row.append(select);
        const asset=decorationAsset(d,o.model);if(asset&&o.paths[asset]){const image=new Image();image.crossOrigin='anonymous';image.onload=()=>{if(!thumb.isConnected)return;const c=d.source.type==='clip'?o.model.clips[d.source.clip]:undefined;const r=c?sourceRect(c,image,0):{x:0,y:0,width:image.width,height:image.height};const s=Math.min(80/r.width,54/r.height);thumb.getContext('2d')!.drawImage(image,r.x,r.y,r.width,r.height,(80-r.width*s)/2,(54-r.height*s)/2,r.width*s,r.height*s);};image.src=o.url(o.paths[asset]);}
        const small=document.createElement('small');small.textContent=`${d.enabled?'启用':'关闭'} · ${d.states===null?'全状态':d.states.join(' / ')||'无状态'} · ${d.speed} 倍`;row.append(small);
        const move=(where:'up'|'down'|'top'|'bottom')=>{const current=orderedDecorations(o.model.decorations!).filter(x=>x.side===side);const index=current.indexOf(d);current.splice(index,1);const dest=where==='top'?current.length:where==='bottom'?0:where==='up'?Math.min(current.length,index+1):Math.max(0,index-1);current.splice(dest,0,d);current.forEach((x,i)=>x.order=i);changed(true);};
        const tools=document.createElement('div');tools.className='actions';for(const [name,key]of [['置顶','top'],['上移','up'],['下移','down'],['置底','bottom']]as const)tools.append(button(name,()=>move(key)));row.append(tools);group.append(row);
      }
      lists.append(group);
    }
    const d=o.model.decorations!.find(d=>d.id===selected);o.selected(d?.id??'');if(!d){form.textContent='添加或选择一个装饰开始编辑。';return;}
    const title=document.createElement('h3');title.textContent=d.name||'装饰参数';form.append(title);
    const text=document.createElement('input');text.value=d.name??'';text.maxLength=160;text.oninput=()=>{d.name=text.value;changed();};label(form,'名称',text);
    const id=document.createElement('small');id.textContent=`编号：${d.id}`;form.append(id);
    const enabled=document.createElement('input');enabled.type='checkbox';enabled.checked=d.enabled;enabled.onchange=()=>{d.enabled=enabled.checked;changed();};label(form,'启用此装饰',enabled,true);
    select(form,'所在分组',[['background','背景（角色后方）'],['foreground','前景（角色前方）']],d.side,v=>{d.side=v as Decoration['side'];changed(true);});
    select(form,'内容来源',[['image','静态图片'],['clip','动画片段']],d.source.type,v=>{d.source=v==='image'?{type:'image',asset:Object.keys(o.assets).find(id=>o.assets[id].kind==='image')??''}:{type:'clip',clip:Object.keys(o.model.clips)[0]??''};changed(true);});
    if(d.source.type==='image')select(form,'图片素材',Object.keys(o.assets).filter(id=>o.assets[id].kind==='image').map(id=>[id,id]),d.source.asset,v=>{d.source={type:'image',asset:v};changed(true);});
    else select(form,'动画片段',Object.keys(o.model.clips).map(id=>[id,id]),d.source.clip,v=>{d.source={type:'clip',clip:v};changed(true);});
    const hint=document.createElement('small');hint.textContent='图集请先创建动画片段。直接选静态图片会显示整张图集。复用主体片段只读取帧序和时长，独立定位、独立播放。';form.append(hint);
    const geo=section(form,'geometry','锚点、位置与大小');
    const presets=document.createElement('div');presets.className='actions';presets.append(button('对齐模型中心',()=>{d.anchor={x:.5,y:.5};d.pivot={x:.5,y:.5};changed(true);}),button('对齐模型底部',()=>{d.anchor={x:.5,y:1};d.pivot={x:.5,y:1};changed(true);}),button('使用 feet 锚点',()=>{d.anchor={...o.model.anchors.feet};d.pivot={x:.5,y:1};changed(true);}));geo.append(presets);
    const grid=document.createElement('div');grid.className='grid-two';geo.append(grid);
    for(const [key,name]of [['anchor','模型相对锚点'],['pivot','装饰自身对齐点']]as const)for(const axis of ['x','y']as const)num(grid,`${name} ${axis}（0—1）`,d[key][axis],0,1,.01,v=>{d[key][axis]=v!;changed();});
    for(const axis of ['x','y']as const)num(grid,`${axis==='x'?'左右':'上下'}偏移（模型像素）`,d.offset[axis],-65536,65536,1,v=>{d.offset[axis]=v!;changed();});
    num(grid,'目标宽度（空白使用原尺寸）',d.width??null,.001,65536,1,v=>{d.width=v;changed();},true);num(grid,'目标高度（空白使用原尺寸）',d.height??null,.001,65536,1,v=>{d.height=v;changed();},true);
    const aspect=document.createElement('input');aspect.type='checkbox';aspect.checked=d.keepAspect;aspect.onchange=()=>{d.keepAspect=aspect.checked;changed();};label(geo,'锁定原始宽高比',aspect,true);
    num(geo,'独立缩放（%，随桌宠整体缩放）',d.scale*100,.1,10000,1,v=>{d.scale=v!/100;changed();});
    const playback=section(form,'playback','播放与适用状态');num(playback,'独立速度（倍）',d.speed,.001,100,.1,v=>{d.speed=v!;changed();});
    const all=document.createElement('input');all.type='checkbox';all.checked=d.states===null;all.onchange=()=>{d.states=all.checked?null:[];changed(true);};label(playback,'全状态通用',all,true);
    if(d.states!==null)for(const mode of ['idle','music','quiet']as const){const checkbox=document.createElement('input');checkbox.type='checkbox';checkbox.checked=d.states.includes(mode);checkbox.onchange=()=>{d.states=checkbox.checked?[...d.states!,mode]:d.states!.filter(s=>s!==mode);changed();};label(playback,mode,checkbox,true);}
    const note=document.createElement('p');note.textContent='隐藏暂停，再显示续播。全状态关闭且未选任何状态时，此装饰不会显示。普通回复不会重置播放。';playback.append(note);
    const tools=document.createElement('div');tools.className='actions';tools.append(button('复制此装饰',()=>{const copy=structuredClone(d);copy.id=crypto.randomUUID();copy.name=(d.name||'装饰')+' 副本';copy.order+=1;o.model.decorations!.push(copy);selected=copy.id;changed(true);}),button('删除此装饰',()=>{o.model.decorations=o.model.decorations!.filter(x=>x.id!==d.id);selected=o.model.decorations[0]?.id??'';changed(true);}));form.append(tools);
  }
  function label(parent:HTMLElement,name:string,input:HTMLElement,check=false){const l=document.createElement('label');if(check){l.className='switch';l.append(input,document.createTextNode(name));}else{l.textContent=name;l.append(input);}parent.append(l);}
  function num(parent:HTMLElement,name:string,value:number|null,min:number,max:number,step:number,change:(n:number|null)=>void,nullable=false){const i=document.createElement('input');i.type='number';i.min=String(min);i.max=String(max);i.step=String(step);i.value=value===null?'':String(value);i.oninput=()=>{const n=i.value===''?null:i.valueAsNumber;if(n!==null&&(!Number.isFinite(n)||n<min||n>max)){i.setCustomValidity(`请输入 ${min} 到 ${max} 的数值`);return;}if(n===null&&!nullable){i.setCustomValidity('请填写数值');return;}i.setCustomValidity('');change(n);};label(parent,name,i);}
  function select(parent:HTMLElement,name:string,items:string[][],value:string,change:(v:string)=>void){const i=document.createElement('select');for(const [v,t]of [['','请选择…'],...items]){const opt=document.createElement('option');opt.value=v;opt.textContent=t;i.append(opt);}i.value=value;i.onchange=()=>change(i.value);label(parent,name,i);}
  function section(parent:HTMLElement,id:string,title:string){const d=document.createElement('details');d.dataset.fold=id;d.open=open.get(id)??true;const s=document.createElement('summary');s.textContent=title;d.append(s);parent.append(d);return d;}
  draw();
}
