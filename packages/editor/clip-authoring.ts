import type {Clip} from '../presentation/model.ts';

export type ImageSize={width:number;height:number};
export type Grid=Required<Pick<Clip['layout'],'columns'|'rows'|'frameCount'|'cellWidth'|'cellHeight'>>;
const integer=(v:number,max:number)=>Number.isSafeInteger(v)&&v>=1&&v<=max;

// Image dimensions determine bounds, not the artist's intended frame boundaries.
export function inferGrid(size:ImageSize,by:'counts'|'cells',a:number,b:number):Grid {
  if(!integer(a,by==='counts'?4096:16384)||!integer(b,by==='counts'?4096:16384))throw Error('请先填写有效的行列数或单帧宽高。');
  const columns=by==='counts'?a:Math.floor(size.width/a),rows=by==='counts'?b:Math.floor(size.height/b);
  const cellWidth=by==='cells'?a:Math.floor(size.width/a),cellHeight=by==='cells'?b:Math.floor(size.height/b);
  if(!integer(columns,4096)||!integer(rows,4096)||!integer(cellWidth,16384)||!integer(cellHeight,16384))throw Error('当前尺寸无法容纳这个网格。');
  if(columns*rows>4096)throw Error('推算网格超过 4096 帧；请手动指定有效帧数，或调整行列/单帧尺寸。');
  return {columns,rows,cellWidth,cellHeight,frameCount:columns*rows};
}

export function parseRects(text:string):NonNullable<Clip['layout']['frames']> {
  const lines=text.trim().split(/\r?\n/).filter(s=>s.trim());
  if(!lines.length||lines.length>4096)throw Error('逐帧矩形需要 1—4096 行，每行填写 x, y, width, height。');
  return lines.map((line,i)=>{
    const parts=line.trim().split(/[,，\s]+/).map(Number);
    if(parts.length!==4||parts.some(n=>!Number.isSafeInteger(n))||parts[0]<0||parts[1]<0||parts[2]<1||parts[3]<1)throw Error(`第 ${i+1} 行不是有效帧矩形。`);
    return {x:parts[0],y:parts[1],width:parts[2],height:parts[3]};
  });
}

export function clipChecks(clip:Clip,size:ImageSize):{errors:string[];warnings:string[]} {
  const errors:string[]=[],warnings:string[]=[],l=clip.layout;
  if(!clip.asset)errors.push('请选择素材库中的图片。');
  if(!Number.isFinite(clip.frameDurationMs)||clip.frameDurationMs<16||clip.frameDurationMs>60000)errors.push('每帧时长须为 16—60000 毫秒。');
  if(!['forward','reverse','ping-pong'].includes(clip.direction))errors.push('请选择有效播放方向。');
  if(l.type==='grid'){
    for(const key of ['columns','rows','frameCount','cellWidth','cellHeight'] as const)if(!integer(l[key]!,key.startsWith('cell')?16384:4096))errors.push(`${key} 必须是范围内的正整数。`);
    if(!errors.length){
      if(l.frameCount!>l.columns!*l.rows!)errors.push('有效帧数超过行列网格容量。');
      if(l.columns!*l.cellWidth!>size.width||l.rows!*l.cellHeight!>size.height)errors.push('网格超出图片边界。');
      if(!errors.length){
        const right=size.width-l.columns!*l.cellWidth!,bottom=size.height-l.rows!*l.cellHeight!;
        if(right||bottom)warnings.push(`图片右侧 ${right} 像素、底部 ${bottom} 像素不参与切帧；如为留白可继续。`);
        if(l.frameCount!<l.columns!*l.rows!)warnings.push(`仅使用前 ${l.frameCount} 格，其余格不播放。`);
      }
    }
  }else if(l.type==='rects'){
    if(!l.frames?.length||l.frames.length>4096)errors.push('帧矩形数量须为 1—4096。');
    for(const [i,f] of (l.frames??[]).entries())if(!Number.isSafeInteger(f.x)||!Number.isSafeInteger(f.y)||f.x<0||f.y<0||!integer(f.width,16384)||!integer(f.height,16384)||f.x+f.width>size.width||f.y+f.height>size.height){errors.push(`第 ${i+1} 帧矩形无效或越过图片边界。`);break;}
  }else if(l.type!=='single')errors.push('不支持的切帧布局。');
  if(clip.direction!=='ping-pong'&&(clip.startReverse||clip.repeatEndpoints||clip.returnToStart))warnings.push('三个往返选项仅在 ping-pong 方向生效，当前方向会忽略它们。');
  if(clip.direction==='ping-pong'&&clip.repeatEndpoints&&clip.returnToStart)warnings.push('repeatEndpoints 已包含回到起点；returnToStart 不再额外追加一帧。');
  return {errors,warnings};
}

export function suggestedClipId(asset:string,existing:string[]):string {
  const base='clip.'+(asset.replace(/^(?:image|visual)\./,'').replace(/[^\p{L}\p{N}_.-]+/gu,'-')||'new');
  let id=base,n=2;while(existing.includes(id))id=`${base}-${n++}`;return id;
}

export function optionalFlags(values:Record<'repeatEndpoints'|'returnToStart'|'startReverse',string>):Partial<Clip>{
  const result:Partial<Clip>={};
  for(const key of ['repeatEndpoints','returnToStart','startReverse'] as const){const v=values[key];if(v==='true'||v==='false')result[key]=v==='true';}
  return result;
}
