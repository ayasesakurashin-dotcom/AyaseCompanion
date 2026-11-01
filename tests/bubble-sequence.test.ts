import test from 'node:test';
import assert from 'node:assert/strict';
import {BubblePresenter,defaultBubblePreferences} from '../packages/presentation/bubble.ts';

function fixture(readMs=1000){
  let clock=0,next=0;const timers=new Map<number,{at:number;fn:()=>void}>();
  const oldWindow=globalThis.window,oldPerformance=globalThis.performance,oldClear=globalThis.clearTimeout;
  Object.defineProperty(globalThis,'performance',{configurable:true,value:{now:()=>clock}});
  globalThis.window={setTimeout:(fn:()=>void,ms:number)=>{const id=++next;timers.set(id,{at:clock+ms,fn});return id;}} as unknown as Window & typeof globalThis;
  globalThis.clearTimeout=((id:number)=>timers.delete(id)) as unknown as typeof clearTimeout;
  class Element {
    textContent='';innerHTML='';dataset:Record<string,string>={};
    style={setProperty:()=>{},width:'',left:'',bottom:'',maxHeight:'',height:''};
    classes=new Set<string>();classList={add:(...names:string[])=>names.forEach(n=>this.classes.add(n)),remove:(...names:string[])=>names.forEach(n=>this.classes.delete(n))};
    getBoundingClientRect(){return {height:Math.max(1,Math.ceil(this.textContent.length/4))*20};}
  }
  const element=new Element(),text=new Element(),counter=new Element(),measure=new Element();
  const root=Object.assign(element,{querySelector:(s:string)=>s==='.bubble-text'?text:s==='.bubble-counter'?counter:measure});
  const bubble=new BubblePresenter(root as unknown as HTMLElement);
  bubble.layout({x:100,y:300,width:100,height:100},{width:600,height:700},1,{...defaultBubblePreferences,animation:'none',readMs});
  return {bubble,root,text,timers,now:()=>clock,step(){const entry=[...timers].sort((a,b)=>a[1].at-b[1].at)[0];assert.ok(entry);timers.delete(entry[0]);clock=entry[1].at;entry[1].fn();},restore(){globalThis.window=oldWindow;globalThis.clearTimeout=oldClear;Object.defineProperty(globalThis,'performance',{configurable:true,value:oldPerformance});}};
}

test('多段文字独立分页与阅读，段间立即交接，仅最后一段关闭并完成',()=>{
  const f=fixture();try{
    let completed=0;const progress:string[]=[];
    f.bubble.startSequence(['abcdefghijklmnop','第二段','last, line'],1000,failed=>{assert.ok(!failed);completed++;},s=>progress.push(s));
    assert.equal(f.text.textContent,'abcdefgh');
    f.step();assert.equal(f.text.textContent,'ijklmnop');assert.equal(completed,0);
    f.step();assert.equal(f.text.textContent,'第二段');assert.equal(f.root.dataset.phase,'reading');assert.equal(completed,0);
    f.step();assert.equal(f.text.textContent,'last, li');assert.equal(completed,0);
    f.step();assert.equal(f.text.textContent,'ne');assert.equal(completed,0);
    f.step();assert.equal(f.root.dataset.phase,'closing');assert.equal(completed,0);
    f.step();assert.equal(completed,1);assert.equal(f.root.dataset.phase,'off');
    assert.equal(progress.at(-1),'abcdefghijklmnop\n第二段\nlast, line');
    assert.ok(progress.every((s,i)=>!i||s.startsWith(progress[i-1])));
  }finally{f.restore();}
});

test('明确取消撤销整组剩余文字，旧计时不能继续播放或完成新组',()=>{
  const f=fixture();try{
    let oldDone=0,newDone=0;
    f.bubble.startSequence(['first','must never show'],1000,()=>oldDone++);
    f.bubble.cancel();assert.equal(f.timers.size,0);
    f.bubble.startSequence(['new'],1000,()=>newDone++);
    while(f.timers.size)f.step();
    assert.equal(oldDone,0);assert.equal(newDone,1);
  }finally{f.restore();}
});
test('每个短素材均遵守全局最少阅读时间，较小的条目readMs不能缩短',()=>{
  const f=fixture(5000);try{
    let done=0;f.bubble.startSequence(['甲','乙'],1000,()=>done++);
    assert.equal(f.now(),0);assert.equal(f.text.textContent,'甲');
    f.step();assert.equal(f.now(),5000);assert.equal(f.text.textContent,'乙');assert.equal(done,0);
    f.step();assert.equal(f.now(),10000);assert.equal(f.root.dataset.phase,'closing');assert.equal(done,0);
    f.step();assert.equal(done,1);
  }finally{f.restore();}
});
