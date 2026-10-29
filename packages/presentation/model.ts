export interface Clip {
  asset: string;
  layout: {type:'single'|'grid'|'rects'; columns?:number; rows?:number; frameCount?:number; cellWidth?:number; cellHeight?:number; frames?:{x:number;y:number;width:number;height:number}[]};
  frameDurationMs:number; frameDurationsMs?:number[];
  direction:'forward'|'reverse'|'ping-pong'; startReverse?:boolean; repeatEndpoints?:boolean; returnToStart?:boolean;
  offset?:{x:number;y:number};
}
export interface Model {
  hitAreas?:{id:string;x:number;y:number;width:number;height:number}[];
  logicalSize:{width:number;height:number};
  anchors:{feet:{x:number;y:number};bubble:{x:number;y:number}};
  clips:Record<string,Clip>;
  baseModes:Record<string,{loop:string;enter?:string;exit?:string}>;
  semanticCues:Record<string,{clip:string;repeat?:number}>;
}
export function frameOrder(clip:Clip):number[] {
  const count=clip.layout.type==='single'?1:clip.layout.type==='grid'?clip.layout.frameCount!:clip.layout.frames!.length;
  let frames=Array.from({length:count},(_,i)=>i);
  if(clip.direction==='reverse'||(clip.direction==='ping-pong'&&clip.startReverse))frames.reverse();
  if(clip.direction==='ping-pong'&&count>1){const back=[...frames].reverse();frames=frames.concat(clip.repeatEndpoints?back:clip.returnToStart?back.slice(1):back.slice(1,-1));}
  return frames;
}
export function timeline(clip:Clip):{frame:number;end:number}[]{let time=0;return frameOrder(clip).map(frame=>({frame,end:time+=(clip.frameDurationsMs?.[frame]??clip.frameDurationMs)}));}
export function frameAt(clip:Clip,elapsed:number,loop:boolean):number {
  const frames=timeline(clip);const duration=frames.at(-1)!.end;
  const time=loop?Math.max(0,elapsed)%duration:Math.min(Math.max(0,elapsed),duration);
  return (frames.find(f=>time<f.end)??frames.at(-1)!).frame;
}
export function paginate(text:string,limit=72):string[]{
  // Array.from counts Unicode code points, avoiding split surrogate pairs.
  const chars=Array.from(text.replace(/\r\n/g,'\n'));
  const pages:string[]=[];for(let i=0;i<chars.length;i+=limit)pages.push(chars.slice(i,i+limit).join(''));
  return pages;
}
