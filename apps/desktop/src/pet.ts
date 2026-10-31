import {invoke,convertFileSrc} from '@tauri-apps/api/core';
import {getCurrentWindow} from '@tauri-apps/api/window';
import {SpriteRenderer} from '../../../packages/presentation/sprite.ts';
import {BubblePresenter,defaultBubblePreferences} from '../../../packages/presentation/bubble.ts';
import {TaskAudioPlayer} from '../../../packages/presentation/audio.ts';
import type {Snapshot,Task,BaseTransition} from './types.ts';

export interface PetBridge {invoke:typeof invoke;assetUrl:(path:string)=>string;drag:()=>Promise<void>}
export function setupPet(root:HTMLElement,bridge:PetBridge={invoke,assetUrl:convertFileSrc,drag:()=>getCurrentWindow().startDragging()}){
  root.innerHTML='<div class="pet-shell"><div class="bubble hidden" aria-live="off"></div><canvas aria-label="角色模型"></canvas><div class="pet-toolbar"><button data-input title="展开独立输入框">输入</button><button data-stop title="停止当前整条回复">停止</button><button data-settings title="打开控制台">设置</button></div><div class="pet-empty">请在控制台导入并激活角色包</div></div>';
  const canvas=root.querySelector('canvas')!;
  let regionSendAt=0,regionBusy=false;
  const regions=async(s:Snapshot)=>{
    root.querySelector<HTMLElement>('.pet-shell')!.dataset.interactive=String(!!s.petInteractive);
    if(regionBusy||performance.now()-regionSendAt<100)return;
    const shell=root.getBoundingClientRect(),box=canvas.getBoundingClientRect(),b=renderer.visibleBounds();
    const rects:{x:number;y:number;width:number;height:number}[]=[];
    if(loaded&&s.model&&b.width>0&&b.height>0)rects.push({x:box.x-shell.x+b.x*box.width/logical.width,y:box.y-shell.y+b.y*box.height/logical.height,width:b.width*box.width/logical.width,height:b.height*box.height/logical.height});
    const toolbar=root.querySelector<HTMLElement>('.pet-toolbar')!.getBoundingClientRect();
    rects.push({x:toolbar.x-shell.x,y:toolbar.y-shell.y,width:toolbar.width,height:toolbar.height});
    if(!s.instance){const empty=root.querySelector<HTMLElement>('.pet-empty')!.getBoundingClientRect();if(empty.width>0&&empty.height>0)rects.push({x:empty.x-shell.x,y:empty.y-shell.y,width:empty.width,height:empty.height});}
    regionBusy=true;regionSendAt=performance.now();
    try{await bridge.invoke('pet_hit_regions',{instanceId:s.instance?.id??null,packageHash:s.instance?.packageHash??s.package?.hash??null,rects});}catch{/* A changed instance or a development preview cannot own native input. */}finally{regionBusy=false;}
  };
  const bubble=new BubblePresenter(root.querySelector<HTMLElement>('.bubble')!);
  const renderer=new SpriteRenderer(canvas);let loaded='',loading=false,current:Task|null=null,epoch=0;
  let baseCurrent:BaseTransition|null=null,baseAckPending=false;
  const audio=new TaskAudioPlayer();let ackPending=false;
  let segmentPlaying=false,audioEnded=false,audioFailed=false,lastProgress=0,lastProgressText='';
  const ack=(task:Task,channel:string,event:string)=>bridge.invoke<boolean>('presentation_ack',{id:task.id,generation:task.generation,channel,event,visualBase:channel==='animation'?canvas.dataset.base:undefined,visualKind:channel==='animation'?canvas.dataset.presentation:undefined});
  const clear=(preserveBase=false)=>{epoch++;bubble.cancel();audio.stop();segmentPlaying=false;audioEnded=false;audioFailed=false;lastProgress=0;lastProgressText='';if(!preserveBase){renderer.cancel();baseCurrent=null;}current=null;};
  const streamAudio=(task:Task)=>{
    if(audioEnded||segmentPlaying||task.channels.audio!=='pending'||!task.plan.audioDone)return;
    const parts=task.plan.audioSegments??[];
    audioFailed=parts.some(part=>!part||part.failed);
    const ready=parts.flatMap((part,index)=>part?.path?[{index,url:bridge.assetUrl(part.path)}]:[]);
    segmentPlaying=true;const own=epoch;
    audio.playSequence(ready,615_000,failed=>{
      if(epoch!==own||current?.id!==task.id)return;
      segmentPlaying=false;audioEnded=true;audioFailed ||= failed;
      void ack(task,'audio',audioFailed?'failed':'completed');
    },(index,event)=>{
      if(epoch===own&&current?.id===task.id)void bridge.invoke('audio_segment_ack',{id:task.id,generation:task.generation,index,event});
    });
  };
  const startBase=async(transition:BaseTransition)=>{
    baseAckPending=true;
    const baseAck=(event:string)=>bridge.invoke<boolean>('base_transition_ack',{id:transition.id,generation:transition.generation,event,visualKind:canvas.dataset.presentation});
    try{
      if(!await baseAck('started'))return;
      baseCurrent=transition;
      const alive=()=>baseCurrent?.id===transition.id&&baseCurrent.generation===transition.generation;
      try{renderer.playFinite(transition.clips,1,()=>{if(alive())void baseAck('completed');},true);}
      catch{if(alive())void baseAck('failed');}
    }finally{baseAckPending=false;}
  };
  const start=async(task:Task)=>{
    ackPending=true;
    try{
      // Authorization precedes any output. Every async continuation additionally
      // checks this local generation, including decode, page and audio callbacks.
      const valid=await ack(task,'','started');if(!valid)return;
      clear();current=task;const own=epoch;const alive=()=>epoch===own&&current?.id===task.id;
      const done=(c:string,failed=false)=>{if(alive())void ack(task,c,failed?'failed':'completed');};
      const plan=task.plan;
      const progress=(text:string)=>{if(!alive())return;const now=performance.now();if(text!==lastProgressText&&(now-lastProgress>=250||text===current?.plan.text)){lastProgress=now;lastProgressText=text;void bridge.invoke('text_progress',{id:task.id,generation:task.generation,text});}};
      if(plan.stream){bubble.startStream(plan.text??'',plan.readMs,failed=>done('text',failed||!!current?.plan.serviceError),progress);bubble.append(plan.text??'',!!plan.streamDone);streamAudio(task);}
      else if(plan.textSegments?.length)bubble.startSequence(plan.textSegments,plan.readMs,failed=>done('text',failed),progress);
      else if(plan.text)bubble.start(plan.text,plan.readMs,failed=>done('text',failed),progress);
      if(!plan.stream&&plan.audioSegments?.length)streamAudio(task);
      if(plan.clips?.length){try{renderer.playFinite(plan.clips,plan.repeat??1,()=>done('animation'),!!task.targetBase);}catch{done('animation',true);}}
      if(plan.audioPath&&task.channels.audio==='pending'){
        // Decode stalls and device failures release only the failed audio channel.
        const duration=Math.min(600_000,Math.max(30_000,(plan.audioDurationMs??150_000)+15_000));
        audio.play(bridge.assetUrl(plan.audioPath),duration,failed=>done('audio',failed));
      }
    }finally{ackPending=false;}
  };
  let drag=false,pointerStart:{x:number;y:number}|null=null,clickTimer=0,clickOwner='';let hitAreas:{id:string;x:number;y:number;width:number;height:number}[]=[];let logical={width:1,height:1};
  canvas.addEventListener('pointerdown',e=>{if(e.button!==0)return;pointerStart={x:e.clientX,y:e.clientY};});
  canvas.addEventListener('pointermove',e=>{if(!pointerStart||drag)return;if(Math.hypot(e.clientX-pointerStart.x,e.clientY-pointerStart.y)>6){pointerStart=null;drag=true;clearTimeout(clickTimer);void bridge.drag().finally(()=>{drag=false;});}});
  canvas.addEventListener('pointerup',e=>{if(!pointerStart||drag)return;pointerStart=null;const box=canvas.getBoundingClientRect(),x=(e.clientX-box.x)/box.width*logical.width,y=(e.clientY-box.y)/box.height*logical.height,bounds=renderer.visibleBounds();if(x<bounds.x||y<bounds.y||x>bounds.x+bounds.width||y>bounds.y+bounds.height)return;const area=hitAreas.find(a=>x/logical.width>=a.x&&y/logical.height>=a.y&&x/logical.width<a.x+a.width&&y/logical.height<a.y+a.height)?.id??'';const owner=clickOwner;if(!owner)return;if(clickTimer){clearTimeout(clickTimer);clickTimer=0;void bridge.invoke('pointer_event',{instanceId:owner,click:'double',area});}else clickTimer=window.setTimeout(()=>{clickTimer=0;if(owner===clickOwner)void bridge.invoke('pointer_event',{instanceId:owner,click:'single',area});},250);});
  canvas.addEventListener('pointercancel',()=>{pointerStart=null;});
  root.querySelector('[data-input]')!.addEventListener('click',()=>void bridge.invoke('show_window',{label:'input'}));
  root.querySelector('[data-settings]')!.addEventListener('click',()=>void bridge.invoke('show_window',{label:'control'}));
  root.querySelector('[data-stop]')!.addEventListener('click',()=>void bridge.invoke('user_control',{action:'stop',value:null}));
  async function update(s:Snapshot){
    hitAreas=s.model?.hitAreas??[];logical=s.model?.logicalSize??{width:1,height:1};
    if(clickOwner!==(s.instance?.id??'')){clearTimeout(clickTimer);clickTimer=0;clickOwner=s.instance?.id??'';}
    const key=s.instance&&s.package?`${s.instance.id}:${s.package.hash}`:'';
    (root.querySelector('.pet-empty') as HTMLElement).hidden=!!key;
    if(!key){if(loaded){clear();renderer.dispose();loaded='';}void regions(s);return;}
    const width=360*s.zoom,height=280*s.zoom;canvas.style.width=`${width}px`;canvas.style.height=`${height}px`;
    if(key!==loaded){if(loading)return;loading=true;clear();
      try{await renderer.load(s.model!,Object.fromEntries(Object.entries(s.assetPaths).map(([id,path])=>[id,bridge.assetUrl(path)])));loaded=key;await bridge.invoke('renderer_ready',{instanceId:s.instance!.id,packageHash:s.package!.hash});}
      catch(e){(root.querySelector('.pet-empty') as HTMLElement).hidden=false;root.querySelector('.pet-empty')!.textContent=String(e);}
      finally{loading=false;}void regions(s);return;
    }
    const reference=renderer.referenceBounds(),box=canvas.getBoundingClientRect(),shell=root.getBoundingClientRect();
    const sx=width/s.model!.logicalSize.width,sy=height/s.model!.logicalSize.height;
    bubble.layout({x:box.x-shell.x+reference.x*sx,y:box.y-shell.y+reference.y*sy,width:reference.width*sx,height:reference.height*sy},{width:shell.width,height:shell.height},s.zoom,s.bubble??defaultBubblePreferences);
    renderer.applyBase(s.scheduler.appliedBase);
    const base=s.scheduler.baseTransition;
    if(baseCurrent&&(!base||base.id!==baseCurrent.id||base.generation!==baseCurrent.generation)){
      renderer.cancel();baseCurrent=null;
    }
    const active=s.scheduler.active;
    if(current&&(!active||active.id!==current.id||active.generation!==current.generation))clear(!!baseCurrent);
    if(current&&(s.scheduler.muted||active?.channels.audio!=='pending'))audio.stop();
    if(current&&active){current=active;if(active.plan.stream){bubble.append(active.plan.text??'',!!active.plan.streamDone);streamAudio(active);}}
    if(active&&!current&&!ackPending&&!loading&&!baseCurrent)await start(active);
    if(base&&!baseCurrent&&!baseAckPending&&!loading)await startBase(base);
    void regions(s);
    // dragging and input visibility never cancel a presentation.
    void drag;
  }
  return update;
}
