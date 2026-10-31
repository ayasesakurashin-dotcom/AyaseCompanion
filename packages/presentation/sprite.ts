import {frameAt,timeline,type Model,type Clip} from './model.ts';
import type {Bounds} from './bubble.ts';

export class SpriteRenderer {
  private images=new Map<string,HTMLImageElement>();
  private epoch=0;private raf=0;private model:Model|null=null;
  private base='idle';private baseStart=0;
  private finite:{clips:string[];repeat:number;start:number;done:()=>void;finished:boolean;holdEnd:boolean}|null=null;
  private visible:Bounds={x:0,y:0,width:0,height:0};
  private boundsCache=new Map<Clip,Map<number,Bounds>>();
  private reference:Bounds={x:0,y:0,width:360,height:280};
  constructor(private canvas:HTMLCanvasElement){}
  async load(model:Model,paths:Record<string,string>){
    this.dispose();const epoch=++this.epoch;this.model=model;
    this.canvas.width=model.logicalSize.width*2;this.canvas.height=model.logicalSize.height*2;
    const ids=[...new Set(Object.values(model.clips).map(c=>c.asset))];
    // Asset URLs use a different origin in WebView2; CORS keeps alpha measurement readable.
    await Promise.all(ids.map(async id=>{const image=new Image();image.crossOrigin='anonymous';image.src=paths[id]??'';
      try{await image.decode();if(epoch===this.epoch)this.images.set(id,image);}catch{/* The importer reports missing optional assets. */}
    }));
    if(epoch!==this.epoch)return;
    const idle=model.clips[model.baseModes.idle.loop];if(!idle||!this.images.has(idle.asset))throw new Error('待机图片无法显示');
    this.reference=this.measureReference(idle);this.visible=this.reference;
    this.baseStart=performance.now();this.loop();
  }
  applyBase(mode:string){if(this.base!==mode){this.base=mode;this.baseStart=performance.now();}}
  playFinite(clips:string[],repeat:number,done:()=>void,holdEnd=false){
    if(!this.model||clips.some(c=>!this.model!.clips[c]||!this.images.has(this.model!.clips[c].asset)))throw new Error('有限动作资源不可用');
    this.finite={clips,repeat,start:performance.now(),done,finished:false,holdEnd};
  }
  referenceBounds(){return this.reference;}
  visibleBounds(){return this.visible;}
  private measureReference(clip:Clip,frame=0):Bounds {
    const model=this.model!,image=this.images.get(clip.asset)!;
    const cached=this.boundsCache.get(clip)?.get(frame);if(cached)return cached;
    const sample=document.createElement('canvas');
    const scale=Math.min(1,512/model.logicalSize.width,512/model.logicalSize.height);
    sample.width=Math.ceil(model.logicalSize.width*scale);sample.height=Math.ceil(model.logicalSize.height*scale);
    const ctx=sample.getContext('2d',{willReadFrequently:true})!;
    const l=clip.layout;
    const r=l.type==='single'?{x:0,y:0,width:image.width,height:image.height}:l.type==='rects'?l.frames![frame]:{x:(frame%l.columns!)*l.cellWidth!,y:Math.floor(frame/l.columns!)*l.cellHeight!,width:l.cellWidth!,height:l.cellHeight!};
    ctx.scale(scale,scale);
    ctx.drawImage(image,r.x,r.y,r.width,r.height,model.logicalSize.width*model.anchors.feet.x-r.width/2+(clip.offset?.x??0),model.logicalSize.height*model.anchors.feet.y-r.height+(clip.offset?.y??0),r.width,r.height);
    const pixels=ctx.getImageData(0,0,sample.width,sample.height).data;
    let left=sample.width,top=sample.height,right=-1,bottom=-1;
    for(let y=0;y<sample.height;y++)for(let x=0;x<sample.width;x++)if(pixels[(y*sample.width+x)*4+3]>16){left=Math.min(left,x);top=Math.min(top,y);right=Math.max(right,x);bottom=Math.max(bottom,y);}
    const bounds=right<left?{x:0,y:0,width:0,height:0}:{x:left/scale,y:top/scale,width:(right-left+1)/scale,height:(bottom-top+1)/scale};if(!this.boundsCache.has(clip))this.boundsCache.set(clip,new Map());this.boundsCache.get(clip)!.set(frame,bounds);return bounds;
  }
  cancel(){this.finite=null;this.baseStart=performance.now();}
  private loop=()=>{
    const model=this.model;if(!model)return;
    const now=performance.now();let clip:Clip|undefined;let frame=0;
    if(this.finite){const f=this.finite;let elapsed=now-f.start;const ids=Array.from({length:f.repeat},()=>f.clips).flat();
      for(let i=0;i<ids.length;i++){const c=model.clips[ids[i]];const duration=timeline(c).at(-1)!.end;
        clip=c;if(elapsed<duration||i===ids.length-1){frame=frameAt(c,elapsed,false);if(elapsed>=duration&&!f.finished){f.finished=true;
          // Ordinary interaction animations return to the applied base immediately;
          // base transitions hold their end until the host commits the target mode.
          if(!f.holdEnd){this.finite=null;this.baseStart=now;clip=model.clips[(model.baseModes[this.base]??model.baseModes.idle).loop];frame=0;}
          const epoch=this.epoch;queueMicrotask(()=>{if(epoch===this.epoch)f.done();});
        }break;}elapsed-=duration;
      }
    }else{clip=model.clips[(model.baseModes[this.base]??model.baseModes.idle).loop];if(clip)frame=frameAt(clip,now-this.baseStart,true);}
    this.canvas.dataset.presentation=this.finite?'finite':'base';this.canvas.dataset.base=this.base;
    if(clip){this.visible=this.measureReference(clip,frame);this.draw(clip,frame);}
    this.raf=requestAnimationFrame(this.loop);
  };
  private draw(c:Clip,frame:number){
    const im=this.images.get(c.asset),model=this.model;if(!im||!model)return;
    const ctx=this.canvas.getContext('2d')!;const l=c.layout;
    const r=l.type==='single'?{x:0,y:0,width:im.width,height:im.height}:l.type==='rects'?l.frames![frame]:{x:(frame%l.columns!)*l.cellWidth!,y:Math.floor(frame/l.columns!)*l.cellHeight!,width:l.cellWidth!,height:l.cellHeight!};
    ctx.setTransform(2,0,0,2,0,0);ctx.clearRect(0,0,model.logicalSize.width,model.logicalSize.height);ctx.imageSmoothingEnabled=true;
    const x=model.logicalSize.width*model.anchors.feet.x-r.width/2+(c.offset?.x??0);
    const y=model.logicalSize.height*model.anchors.feet.y-r.height+(c.offset?.y??0);
    ctx.drawImage(im,r.x,r.y,r.width,r.height,x,y,r.width,r.height);
  }
  dispose(){++this.epoch;cancelAnimationFrame(this.raf);this.finite=null;this.images.clear();this.boundsCache.clear();this.visible={x:0,y:0,width:0,height:0};this.model=null;this.base='idle';}
}
