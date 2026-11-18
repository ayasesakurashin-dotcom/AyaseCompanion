import {frameAt,timeline,type Clip,type Model} from './model.ts';
import type {Bounds} from './bubble.ts';

export interface Decoration {
  id:string; name?:string; side:'background'|'foreground'; order:number; enabled:boolean;
  source:{type:'image';asset:string}|{type:'clip';clip:string};
  anchor:{x:number;y:number}; pivot:{x:number;y:number}; offset:{x:number;y:number};
  width?:number|null;height?:number|null;keepAspect:boolean;scale:number;speed:number;
  states:('idle'|'music'|'quiet')[]|null;
}
export const decorationDefaults=(id:string,side:Decoration['side']):Decoration=>({id,name:'新装饰',side,order:0,enabled:true,source:{type:'image',asset:''},anchor:{x:.5,y:1},pivot:{x:.5,y:1},offset:{x:0,y:0},width:null,height:null,keepAspect:true,scale:1,speed:1,states:null});
export function orderedDecorations(layers:Decoration[]){return layers.map((layer,index)=>({layer,index})).sort((a,b)=>(a.layer.side==='background'?0:1)-(b.layer.side==='background'?0:1)||a.layer.order-b.layer.order||a.index-b.index).map(x=>x.layer);}
export function decorationVisible(d:Decoration,base:string){return d.enabled&&(d.states===null||d.states.includes(base as 'idle'));}
export function decorationAsset(d:Decoration,model:Model){return d.source.type==='image'?d.source.asset:model.clips[d.source.clip]?.asset;}
export function sourceRect(c:Clip,image:{width:number;height:number},frame:number):Bounds {
  const l=c.layout;return l.type==='single'?{x:0,y:0,width:image.width,height:image.height}:l.type==='rects'?l.frames![frame]:{x:frame%l.columns!*l.cellWidth!,y:Math.floor(frame/l.columns!)*l.cellHeight!,width:l.cellWidth!,height:l.cellHeight!};
}
export function decorationSize(d:Decoration,model:Model,image:{width:number;height:number}){
  const c=d.source.type==='clip'?model.clips[d.source.clip]:undefined;
  if(!c||c.layout.type==='single')return {width:image.width,height:image.height};
  if(c.layout.type==='grid')return {width:c.layout.cellWidth!,height:c.layout.cellHeight!};
  return {width:Math.max(...c.layout.frames!.map(f=>f.width)),height:Math.max(...c.layout.frames!.map(f=>f.height))};
}
export function decorationBounds(d:Decoration,logical:Model['logicalSize'],size:{width:number;height:number}):Bounds {
  let width=d.width??size.width,height=d.height??size.height;
  if(d.keepAspect){const s=d.width&&d.height?Math.min(d.width/size.width,d.height/size.height):d.width?d.width/size.width:d.height?d.height/size.height:1;width=size.width*s;height=size.height*s;}
  width*=d.scale;height*=d.scale;
  return {x:logical.width*d.anchor.x+d.offset.x-width*d.pivot.x,y:logical.height*d.anchor.y+d.offset.y-height*d.pivot.y,width,height};
}
export function unionBounds(a:Bounds,b:Bounds):Bounds {const x=Math.min(a.x,b.x),y=Math.min(a.y,b.y);return {x,y,width:Math.max(a.x+a.width,b.x+b.width)-x,height:Math.max(a.y+a.height,b.y+b.height)-y};}
export function clipBounds(a:Bounds,b:Bounds):Bounds {const x=Math.max(a.x,b.x),y=Math.max(a.y,b.y);return {x,y,width:Math.max(0,Math.min(a.x+a.width,b.x+b.width)-x),height:Math.max(0,Math.min(a.y+a.height,b.y+b.height)-y)};}

// Visibility fades have their own clock: hiding freezes animation immediately,
// while the last frame can remain on screen until its opacity reaches zero.
export class DecorationFades {
  private tracks=new Map<string,{opacity:number;from:number;target:number;start:number}>();
  sync(layers:Decoration[],base:string,now:number){
    this.tick(now);
    const ids=new Set(layers.map(d=>d.id));for(const id of this.tracks.keys())if(!ids.has(id))this.tracks.delete(id);
    for(const d of layers){
      const target=decorationVisible(d,base)?1:0,t=this.tracks.get(d.id);
      // New scene/layer contents are shown directly, without resurrecting a removed layer.
      if(!t)this.tracks.set(d.id,{opacity:target,from:target,target,start:now});
      else if(t.target!==target){t.from=t.opacity;t.target=target;t.start=now;}
    }
  }
  tick(now:number){
    let hidden=false;
    for(const t of this.tracks.values()){
      const before=t.opacity,progress=Math.min(1,Math.max(0,now-t.start)/200);
      t.opacity=t.from+(t.target-t.from)*progress;
      if(before>0&&t.opacity===0)hidden=true;
    }
    return hidden;
  }
  opacity(id:string){return this.tracks.get(id)?.opacity??0;}
  clear(){this.tracks.clear();}
}

// One scene tick advances independent tracks. Hidden tracks retain their phase.
export class DecorationClock {
  private tracks=new Map<string,{elapsed:number;last:number;active:boolean;speed:number;source:string;frames:{frame:number;end:number}[]}>();
  sync(layers:Decoration[],model:Model,base:string,now:number,visible=true){
    this.tick(now);
    const ids=new Set(layers.map(d=>d.id));for(const id of this.tracks.keys())if(!ids.has(id))this.tracks.delete(id);
    for(const d of layers){
      const clip=d.source.type==='clip'?model.clips[d.source.clip]:undefined;
      const source=JSON.stringify(clip?[d.source,{...clip,offset:undefined}]:d.source);
      let t=this.tracks.get(d.id);if(!t||t.source!==source){t={elapsed:0,last:now,active:false,speed:d.speed,source,frames:clip?timeline(clip):[]};this.tracks.set(d.id,t);}
      t.last=now;t.speed=d.speed;t.active=visible&&decorationVisible(d,base);
    }
  }
  tick(now:number){for(const t of this.tracks.values()){if(t.active)t.elapsed+=Math.max(0,now-t.last)*t.speed;t.last=now;const duration=t.frames.at(-1)?.end;if(duration)t.elapsed%=duration;}}
  frame(id:string,clip:Clip){const t=this.tracks.get(id);return t?.frames.length?(t.frames.find(f=>t.elapsed<f.end)??t.frames.at(-1)!).frame:frameAt(clip,0,true);}
  elapsed(id:string){return this.tracks.get(id)?.elapsed??0;}
  clear(){this.tracks.clear();}
}
