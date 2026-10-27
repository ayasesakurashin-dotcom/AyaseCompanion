import test from 'node:test';
import assert from 'node:assert/strict';
import {frameOrder,frameAt,timeline,paginate,type Clip} from '../packages/presentation/model.ts';
const clip:Clip={asset:'test',layout:{type:'grid',columns:4,rows:1,frameCount:4,cellWidth:1,cellHeight:1},frameDurationMs:100,direction:'forward'};
test('正放、倒放和往返有限片段保持帧顺序',()=>{
  assert.deepEqual(frameOrder(clip),[0,1,2,3]);assert.deepEqual(frameOrder({...clip,direction:'reverse'}),[3,2,1,0]);
  assert.deepEqual(frameOrder({...clip,direction:'ping-pong'}),[0,1,2,3,2,1]);
  assert.deepEqual(frameOrder({...clip,direction:'ping-pong',returnToStart:true}),[0,1,2,3,2,1,0]);
  assert.deepEqual(frameOrder({...clip,direction:'ping-pong',repeatEndpoints:true}),[0,1,2,3,3,2,1,0]);
  assert.deepEqual(frameOrder({...clip,direction:'ping-pong',startReverse:true,returnToStart:true}),[3,2,1,0,1,2,3]);
});
test('有限片段保留末帧，循环片段恢复首帧',()=>{assert.equal(frameAt(clip,400,false),3);assert.equal(frameAt(clip,400,true),0);assert.equal(frameAt(clip,100,false),1);});
test('逐帧时长跟随源帧，倒放不改变帧与时长配对',()=>{const c={...clip,direction:'reverse' as const,frameDurationsMs:[100,200,300,400]};assert.deepEqual(timeline(c).map(f=>f.end),[400,700,900,1000]);});
test('中文、换行及 emoji 分页不丢字、不重复、不截断代理对',()=>{const text=('中文😀测试\n').repeat(43);const pages=paginate(text);assert.equal(pages.join(''),text);assert.ok(pages.every(p=>Array.from(p).length<=72));assert.ok(pages.every(p=>!/[\uD800-\uDBFF]$/.test(p)));});
test('只有静态 idle 时也能播放，不形成无限有限任务',()=>{const c={...clip,layout:{type:'single' as const},direction:'ping-pong' as const};assert.deepEqual(frameOrder(c),[0]);assert.equal(frameAt(c,99999,true),0);});
