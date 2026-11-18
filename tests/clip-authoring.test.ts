import test from 'node:test';
import assert from 'node:assert/strict';
import {clipChecks,inferGrid,optionalFlags,parseRects,suggestedClipId} from '../packages/editor/clip-authoring.ts';
import {frameOrder,type Clip} from '../packages/presentation/model.ts';
const size={width:1592,height:355};
const grid:Clip={asset:'border',layout:{type:'grid',columns:8,rows:5,cellWidth:199,cellHeight:71,frameCount:40},frameDurationMs:200,direction:'forward'};

test('按行列或单帧尺寸推算 Ema 网格，只推算几何而非识别帧',()=>{
 const expected={columns:8,rows:5,cellWidth:199,cellHeight:71,frameCount:40};
 assert.deepEqual(inferGrid(size,'counts',8,5),expected);assert.deepEqual(inferGrid(size,'cells',199,71),expected);
 // Several layouts are geometrically valid. The user must inspect the artwork.
 assert.equal(clipChecks({...grid,layout:{type:'grid',...inferGrid(size,'counts',4,5)}},size).errors.length,0);
 assert.throws(()=>inferGrid(size,'counts',0,5));assert.throws(()=>inferGrid(size,'cells',2000,71));assert.throws(()=>inferGrid(size,'counts',100,100));
});
test('区分越界/容量错误与合法留白、末行未用格',()=>{
 assert.deepEqual(clipChecks(grid,size),{errors:[],warnings:[]});
 const partial=clipChecks({...grid,layout:{...grid.layout,frameCount:39}},size);assert.equal(partial.errors.length,0);assert.equal(partial.warnings.length,1);
 const pad=clipChecks(grid,{width:1600,height:360});assert.equal(pad.errors.length,0);assert.match(pad.warnings[0],/8 像素.*5 像素/);
 for(const change of [{frameCount:41},{cellWidth:200},{columns:0},{rows:1.5},{cellHeight:NaN}])assert.ok(clipChecks({...grid,layout:{...grid.layout,...change}},size).errors.length);
 assert.ok(clipChecks({...grid,frameDurationMs:0},size).errors.length);
});
test('静态不需要切片，逐帧矩形允许不同尺寸和重复帧',()=>{
 assert.equal(clipChecks({asset:'image',layout:{type:'single'},frameDurationMs:1000,direction:'forward'},size).errors.length,0);
 const frames=parseRects('0,0,199,71\n199，0，100，71\n0 0 199 71');
 assert.equal(frames.length,3);assert.equal(clipChecks({...grid,layout:{type:'rects',frames}},size).errors.length,0);
 assert.throws(()=>parseRects('1,2,0,71'));assert.throws(()=>parseRects('0,0,3.5,5'));
 assert.ok(clipChecks({...grid,layout:{type:'rects',frames:parseRects('1500,0,199,71')}},size).errors.length);
});
test('可选布尔项未指定时省略，显式 false 保留并沿用帧序',()=>{
 assert.deepEqual(optionalFlags({repeatEndpoints:'',returnToStart:'null',startReverse:'false'}),{startReverse:false});
 const flags=optionalFlags({repeatEndpoints:'false',returnToStart:'true',startReverse:'true'});
 const clip={...grid,layout:{...grid.layout,frameCount:3},direction:'ping-pong' as const,...flags};assert.deepEqual(frameOrder(clip),[2,1,0,1,2]);
 assert.equal(suggestedClipId('image.border',['clip.border','clip.border-2']),'clip.border-3');
});
