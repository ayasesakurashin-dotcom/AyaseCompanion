import test from 'node:test';
import assert from 'node:assert/strict';
import {bubbleGeometry,graphemes,measuredPages,revealCount,readingTime,defaultBubblePreferences} from '../packages/presentation/bubble.ts';

test('气泡保持头顶间距、只向上增长且不超过可见模型高度',()=>{
  for(const zoom of [.6,1,1.6]){
    const reference={x:100,y:260*zoom,width:130*zoom,height:210*zoom};
    const viewport={width:640,height:900};
    const g=bubbleGeometry(reference,viewport,zoom);
    assert.ok(Math.abs((viewport.height-g.bottom)-(reference.y-12*zoom))<1e-6);
    assert.ok(g.maxHeight<=reference.height);
    assert.ok(reference.y-12*zoom-g.maxHeight>=12);
    assert.ok(g.center-g.width/2>=12&&g.center+g.width/2<=viewport.width-12);
  }
});
test('大小与屏幕边界决定容量，字体不随模型缩放',()=>{
  const small=bubbleGeometry({x:0,y:200,width:90,height:130},{width:420,height:440},.6);
  const big=bubbleGeometry({x:0,y:400,width:240,height:340},{width:620,height:820},1.6);
  assert.ok(big.width>small.width&&big.maxHeight>small.maxHeight);
  const edge=bubbleGeometry({x:600,y:58,width:240,height:340},{width:620,height:520},1);
  assert.equal(edge.maxHeight,34);assert.ok(edge.center+edge.width/2<=608);
});
test('实际测量分页保留换行、空行、组合字符与全部原文',()=>{
  const source=('中文👨‍👩‍👧‍👦é\n\n很长的abcdefghijk。').repeat(8);
  const chars=graphemes(source);
  const measure=(capacity:number)=>(text:string)=>graphemes(text).reduce((n,c)=>n+(c==='\n'?6:1),0)<=capacity;
  const small=measuredPages(chars,measure(30)),large=measuredPages(chars,measure(70));
  assert.equal(small.join(''),source);assert.equal(large.join(''),source);assert.ok(large.length<small.length);
  assert.ok(small.every(p=>!p.endsWith('\u200d')&&!p.startsWith('\u0301')));
});
test('打字机按字形簇显现，阅读计时独立于显现时间',()=>{
  assert.deepEqual(graphemes('A👨‍👩‍👧‍👦é'),['A','👨‍👩‍👧‍👦','é']);
  assert.equal(revealCount(0,10,defaultBubblePreferences),0);
  assert.equal(revealCount(100,10,defaultBubblePreferences),3);
  assert.equal(revealCount(1000,10,defaultBubblePreferences),10);
  assert.equal(revealCount(0,10,{...defaultBubblePreferences,animation:'none'}),10);
  assert.equal(readingTime(10,defaultBubblePreferences),2800);
  assert.equal(readingTime(80,defaultBubblePreferences,1000),5600);
});
