import type {Model} from '../../../packages/presentation/model.ts';

type Asset={kind:string;path:string};
export const assetCategories=[
 {kind:'image',name:'图片与图集',importLabel:'导入图片',extensions:['png','webp'],hint:'PNG / WebP，图集显示首帧缩略图'},
 {kind:'audio',name:'固定音声',importLabel:'导入音声',extensions:['wav','mp3','ogg','flac'],hint:'WAV / MP3 / OGG / FLAC'},
 {kind:'text',name:'固定文字',importLabel:'导入文字',extensions:['txt'],hint:'UTF-8 文本，展开分类后显示内容摘要'},
 {kind:'data',name:'其他素材',importLabel:'导入其他素材',extensions:['json','bin','dat'],hint:'其他附带文件'},
] as const;
export interface AssetLibraryOptions{
 assets:Record<string,Asset>;paths:Record<string,string>;files:{path:string;bytes:number}[];model?:Model|null;
 selected:string;filter:string;scope:string;openState:Map<string,boolean>;cache:Map<string,string>;
 assetUrl:(path:string)=>string;readText:(id:string)=>Promise<string>;
}
const size=(bytes:number)=>bytes<1024?`${bytes} B`:bytes<1024*1024?`${(bytes/1024).toFixed(1)} KB`:`${(bytes/1024/1024).toFixed(1)} MB`;

/** Read-only cards: selecting, importing, auditioning and editing keep using the
 * existing workbench commands. Folding does not alter any authoring data. */
export function renderAssetLibrary(container:HTMLElement,options:AssetLibraryOptions){
 container.querySelectorAll<HTMLDetailsElement>('[data-asset-group]').forEach(d=>options.openState.set(d.dataset.assetGroup!,d.open));
 container.replaceChildren();
 const rows=Object.entries(options.assets),query=options.filter.trim().toLocaleLowerCase();
 const fileSizes=new Map(options.files.map(f=>[f.path,f.bytes]));
 const textJobs:{id:string;node:HTMLElement;key:string}[]=[];let pumping=false;
 async function pump(){if(pumping)return;pumping=true;try{await Promise.all([worker(),worker()]);}finally{pumping=false;}}
 async function worker(){while(textJobs.length){const job=textJobs.shift()!;if(!job.node.isConnected)continue;try{const content=(await options.readText(job.id)).replace(/\s+/g,' ').trim().slice(0,100)||'（空文字素材）';options.cache.set(job.key,content);if(job.node.isConnected)job.node.textContent=content;}catch{if(job.node.isConnected)job.node.textContent='选择素材以查看正文';}}}
 let matches=0;
 for(const category of assetCategories){
  const entries=rows.filter(([,a])=>(assetCategories.some(c=>c.kind===a.kind)?a.kind:'data')===category.kind);
  const shown=entries.filter(([id,a])=>!query||`${id} ${a.path}`.toLocaleLowerCase().includes(query));matches+=shown.length;
  const group=document.createElement('details');group.className='asset-category';group.dataset.assetGroup=category.kind;
  group.open=options.openState.get(category.kind)??(category.kind==='image'||entries.some(([id])=>id===options.selected));
  const summary=document.createElement('summary'),name=document.createElement('strong'),count=document.createElement('span');
  name.textContent=category.name;count.textContent=query?`${shown.length} / ${entries.length} 项`:`${entries.length} 项`;summary.append(name,count);group.append(summary);
  const body=document.createElement('div');body.className='asset-category-body';const tools=document.createElement('div');tools.className='asset-category-tools';
  const hint=document.createElement('small');hint.textContent=category.hint;
  const add=document.createElement('button');add.type='button';add.textContent=category.importLabel;add.dataset.studio='add-assets';add.dataset.assetKind=category.kind;tools.append(hint,add);body.append(tools);
  const grid=document.createElement('div');grid.className='asset-card-grid';body.append(grid);
  for(const[id,asset]of shown){
   const button=document.createElement('button');button.type='button';button.className='asset-card';button.dataset.asset=id;button.setAttribute('aria-pressed',String(id===options.selected));button.title=`${id}\n${asset.path}`;
   const visual=document.createElement('span');visual.className='asset-card-visual';const cacheKey=`${options.scope}:${id}`;
   if(asset.kind==='image'&&options.paths[id]){
    const img=document.createElement('img');img.alt=`${id} 缩略图`;img.loading='lazy';img.decoding='async';img.crossOrigin='anonymous';
    const cached=options.cache.get(cacheKey);img.src=cached??options.assetUrl(options.paths[id]);
    if(!cached)img.onload=()=>{img.onload=null;if(!img.isConnected)return;const clip=Object.values(options.model?.clips??{}).find(c=>c.asset===id),layout=clip?.layout;
     const frame=layout?.type==='grid'?{x:0,y:0,width:layout.cellWidth!,height:layout.cellHeight!}:layout?.type==='rects'?layout.frames?.[0]:undefined;
     const r=frame??{x:0,y:0,width:img.naturalWidth,height:img.naturalHeight};if(!r.width||!r.height||r.x<0||r.y<0||r.x+r.width>img.naturalWidth||r.y+r.height>img.naturalHeight)return;
     try{const canvas=document.createElement('canvas');canvas.width=168;canvas.height=144;const scale=Math.min(160/r.width,136/r.height),w=r.width*scale,h=r.height*scale;canvas.getContext('2d')!.drawImage(img,r.x,r.y,r.width,r.height,(168-w)/2,(144-h)/2,w,h);const thumb=canvas.toDataURL('image/png');options.cache.set(cacheKey,thumb);img.src=thumb;}catch{/* An unavailable image keeps its regular file preview. */}
    };
    img.onerror=()=>{img.hidden=true;visual.classList.add('asset-image-missing');visual.setAttribute('aria-label','图片预览不可用');};visual.append(img);
   }else{visual.classList.add('asset-kind-mark');visual.setAttribute('aria-hidden','true');visual.textContent=asset.kind==='audio'?'♪':asset.kind==='text'?'文':'档';}
   const title=document.createElement('strong');title.className='asset-card-name';title.textContent=id;
   const meta=document.createElement('small');meta.className='asset-card-meta';const bytes=fileSizes.get(asset.path);meta.textContent=[asset.path.split('.').at(-1)?.toUpperCase(),bytes===undefined?'':size(bytes)].filter(Boolean).join(' · ');
   button.append(visual,title,meta);
   if(asset.kind==='text'){const excerpt=document.createElement('span');excerpt.className='asset-card-excerpt';excerpt.textContent=options.cache.get(cacheKey)??'展开分类后加载摘要';if(!options.cache.has(cacheKey))textJobs.push({id,node:excerpt,key:cacheKey});button.append(excerpt);}
   grid.append(button);
  }
  if(!shown.length){const empty=document.createElement('p');empty.className='asset-category-empty';empty.textContent=query?'此分类没有匹配的素材。':'尚未导入此类素材。';grid.append(empty);}
  group.append(body);container.append(group);
  const queueVisible=()=>{options.openState.set(category.kind,group.open);if(group.open&&category.kind==='text')void pump();};group.ontoggle=queueVisible;
  if(group.open&&category.kind==='text')void pump();
 }
 if(query&&!matches){const empty=document.createElement('p');empty.className='hint';empty.textContent='未找到匹配的素材编号或文件名。';container.prepend(empty);}
}
