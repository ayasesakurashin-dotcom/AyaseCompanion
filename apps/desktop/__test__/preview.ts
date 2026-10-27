import {setupPet} from '../src/pet.ts';
import {defaultBubblePreferences,measuredPages,graphemes} from '../../../packages/presentation/bubble.ts';
import type {Snapshot,Task} from '../src/types.ts';
import '../src/style.css';
if(!import.meta.env.DEV)throw new Error('Development regression page only');
const fixture=await fetch('./local-fixture.json').then(r=>{if(!r.ok)throw Error('请先运行 tools/create-preview-fixture.py');return r.json();});
const root=document.querySelector<HTMLElement>('#stage')!,status=document.querySelector('#status')!,result=document.querySelector('#result')!,receipts=document.querySelector('#receipts')!;
let serial=0,completed=false,busy=false;
const log:{at:number;event:string;channel:string;presentation?:string}[]=[];
const state={...fixture,version:'0.1.0',stage:'browser regression',instance:{id:'preview',name:'Ema'},instances:[],package:{hash:'preview',id:'preview'},library:[],plugins:[],configured:[],unknownPresets:[],facts:{},zoom:1,bubble:{...defaultBubblePreferences},excluded:[],dataDirectory:'test-only',scheduler:{active:null,tasks:[],desiredBase:'idle',appliedBase:'idle',suppressed:false,muted:false,log:[],claims:[]}} as Snapshot;
const update=setupPet(root,{assetUrl:p=>p,drag:async()=>{},invoke:async(command,args:any)=>{
  if(command==='presentation_ack'){
    const t=state.scheduler.active;
    if(!t||t.id!==args.id||t.generation!==args.generation)return false as any;
    log.push({at:performance.now(),event:args.event,channel:args.channel,presentation:root.querySelector('canvas')?.dataset.presentation});
    if(args.event==='started')t.status='presenting';else t.channels[args.channel]=args.event;
    if(Object.values(t.channels).every(v=>v!=='pending')){t.status='completed';state.scheduler.active=null;completed=true;}
    return true as any;
  }
  if(command==='user_control'){state.scheduler.active=null;completed=true;}
  return undefined as any;
}});
function resize(){root.style.width=`${Math.max(420,360*state.zoom+40)}px`;root.style.height=`${68+560*state.zoom}px`;}
resize();
setInterval(async()=>{if(busy)return;busy=true;try{await update(state);status.textContent=`任务 ${state.scheduler.active?.status??'空闲'} / ${root.querySelector('canvas')?.dataset.presentation} / ${root.querySelector('.bubble')?.getAttribute('data-phase')}`;receipts.textContent=JSON.stringify(log.slice(-8),null,2);}finally{busy=false;}},40);
function start(text:string,clips?:string[],readMs=1000,holdAudio=false){
  completed=false;log.length=0;
  state.scheduler.active={id:++serial,generation:serial,instanceId:'preview',owner:'test',priority:'normal',status:'starting',plan:{text,clips,readMs},channels:{text:'pending',...(clips?{animation:'pending'}:{}),...(holdAudio?{audio:'pending'}:{})}} as Task;
}
const long='你好，我是离线验证中的桌面伙伴。你可以在我说话时打开全屏应用，观察这条回复是否完整结束。\n测试较长文字\n字。 。 。 。 。 。 。 。 。 。 。 。\n\n中文与 English 混合，emoji 👨‍👩‍👧‍👦、组合字 é 不应被切断。'.repeat(2);
document.querySelector('#short')!.addEventListener('click',()=>start('下午好！没有勉强自己吧？',undefined,10000));
document.querySelector('#long')!.addEventListener('click',()=>start(long,undefined,10000));
document.querySelector('#combined')!.addEventListener('click',()=>start(long,[fixture.model.semanticCues.greeting.clip],1000));
document.querySelector('#stop')!.addEventListener('click',()=>{state.scheduler.active=null;});
document.querySelector<HTMLSelectElement>('#zoom')!.onchange=e=>{state.zoom=Number((e.target as HTMLSelectElement).value);resize();};
document.querySelector<HTMLSelectElement>('#font')!.onchange=e=>{state.bubble.fontSize=Number((e.target as HTMLSelectElement).value);};
document.querySelector<HTMLSelectElement>('#animation')!.onchange=e=>{state.bubble.animation=(e.target as HTMLSelectElement).value as 'none'|'typewriter';};
const wait=(ms:number)=>new Promise(r=>setTimeout(r,ms));
const until=async(predicate:()=>boolean,ms=15000)=>{const end=performance.now()+ms;while(!predicate()){if(performance.now()>end)throw Error('等待超时');await wait(40);}};
const assert=(ok:boolean,message:string)=>{if(!ok)throw Error(message);};
document.querySelector('#suite')!.addEventListener('click',async()=>{
  const button=document.querySelector<HTMLButtonElement>('#suite')!;button.disabled=true;
  const checks:string[]=[];
  const pass=(name:string)=>{checks.push(name);result.textContent=JSON.stringify({status:'running',checks},null,2);};
  try{
    await until(()=>!!root.querySelector('canvas')?.dataset.presentation);
    state.bubble.animation='none';
    const capacities:number[]=[];
    for(const zoom of [.6,1,1.6])for(const fontSize of [15,24]){
      state.zoom=zoom;state.bubble.fontSize=fontSize;resize();start(long,undefined,10000);
      await until(()=>root.querySelector('.bubble')?.getAttribute('data-phase')==='reading');await wait(100);
      const bubble=root.querySelector<HTMLElement>('.bubble')!,measure=root.querySelector<HTMLElement>('.bubble-measure')!;
      const b=bubble.getBoundingClientRect(),stage=root.getBoundingClientRect(),p=root.querySelector<HTMLElement>('.bubble-text')!.getBoundingClientRect();
      assert(b.top>=stage.top+11&&b.left>=stage.left+11&&b.right<=stage.right-11,'气泡超出窗口');
      assert(b.height<=parseFloat(bubble.style.maxHeight)+1,'高度超出限制');
      assert(p.bottom<=b.bottom-20,'文字溢出');
      const pages=measuredPages(graphemes(long),text=>{measure.textContent=text;return measure.getBoundingClientRect().height<=parseFloat(bubble.style.maxHeight)-44+.1;});
      assert(pages.join('')===long,'分页丢字');if(fontSize===15)capacities.push(graphemes(pages[0]).length);
      const bottom=b.bottom;start('短回复',undefined,10000);await wait(180);
      assert(Math.abs(root.querySelector('.bubble')!.getBoundingClientRect().bottom-bottom)<1,'短长文底边改变');
      pass(`布局 ${zoom} / 字号 ${fontSize}`);
    }
    assert(capacities[2]>capacities[0],'放大后容量没有增加');pass('放大气泡增加单页容量');
    state.zoom=1;state.bubble.fontSize=15;state.bubble.animation='typewriter';state.bubble.charactersPerSecond=10;resize();
    start('逐字输出检查',undefined,1000,true);await wait(220);
    const early=root.querySelector('.bubble-text')!.textContent!;assert(early.length>0&&early.length<6,'未逐字显现');
    await until(()=>log.some(x=>x.channel==='text'&&x.event==='completed'));
    assert(root.querySelector('.bubble')!.classList.contains('hidden'),'读完未关闭');
    assert(state.scheduler.active?.channels.audio==='pending','文本关闭错误释放其他通道');pass('逐字显现、读后关闭且等待剩余音声');
    state.bubble.animation='none';start(long,[fixture.model.semanticCues.greeting.clip],10000);
    await until(()=>log.some(x=>x.channel==='animation'&&x.event==='completed'));
    assert(root.querySelector('canvas')?.dataset.presentation==='base','动作完成仍停在末帧');
    assert(state.scheduler.active?.channels.text==='pending','动画提前释放任务');pass('动画提前回基础姿态、任务继续等文字');
    state.scheduler.active=null;await wait(160);assert(root.querySelector('.bubble')!.classList.contains('hidden'),'停止后气泡未关闭');pass('停止清理气泡');
    state.zoom=.6;state.bubble.fontSize=24;state.bubble.animation='typewriter';state.bubble.charactersPerSecond=30;resize();
    const resizeText='缩放时这条中文回复仍应完整可读，包含组合字 é 与家庭表情 👨‍👩‍👧‍👦。';
    start(resizeText);await wait(160);state.zoom=1.6;state.bubble.fontSize=15;resize();
    await until(()=>root.querySelector('.bubble')?.getAttribute('data-phase')==='reading');
    assert(root.querySelector('.bubble-text')!.textContent===resizeText,'缩放后丢字');
    await until(()=>completed);pass('打字途中缩放与改字号不丢字');
    root.style.height='100px';start('窗口过小必须有界失败');
    await until(()=>completed);
    assert(log.some(x=>x.channel==='text'&&x.event==='failed'),'空间不足没有明确失败');pass('过小可用空间不永久占锁');
    state.zoom=1;state.bubble.animation='none';resize();
    result.textContent=JSON.stringify({status:'passed',checks,capacities},null,2);
  }catch(error){result.textContent=JSON.stringify({status:'failed',checks,error:String(error)},null,2);}finally{button.disabled=false;}
});
