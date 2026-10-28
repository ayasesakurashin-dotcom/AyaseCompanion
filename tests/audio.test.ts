import test from 'node:test';
import assert from 'node:assert/strict';
import {TaskAudioPlayer,type AudioBackend} from '../packages/presentation/audio.ts';

const flush=()=>new Promise(resolve=>setImmediate(resolve));
function deferred<T>() {
  let resolve!:(value:T)=>void;
  const promise=new Promise<T>(r=>{resolve=r;});
  return {promise,resolve};
}
function fixture() {
  const sources:{started:boolean;stopped:boolean;disconnected:boolean;onended:(()=>void)|null}[]=[];
  let closed=0,signal:AbortSignal|undefined;
  const response={ok:true,arrayBuffer:async()=>new ArrayBuffer(8)};
  const backend:AudioBackend={
    fetch:async(_url,options)=>{signal=options.signal;return response;},
    createContext:()=>({
      destination:{},resume:async()=>{},decodeAudioData:async()=>({length:4000,numberOfChannels:1,duration:.5}),
      close:async()=>{closed++;},
      createBufferSource:()=>{
        const source={started:false,stopped:false,disconnected:false,onended:null as (()=>void)|null,buffer:null,
          connect(){},disconnect(){this.disconnected=true;},start(){this.started=true;},stop(){this.stopped=true;}};
        sources.push(source);return source;
      },
    }) as unknown as AudioContext,
  };
  return {backend,sources,response,get closed(){return closed;},get signal(){return signal;}};
}

test('音声仅在实际 ended 后完成，完成后释放音频资源',async()=>{
  const f=fixture(),events:boolean[]=[];const player=new TaskAudioPlayer(f.backend);
  player.play('local-voice',1000,failed=>events.push(failed));await flush();
  assert.equal(f.sources[0].started,true);assert.deepEqual(events,[]);
  const late=f.sources[0].onended!;late();late();await flush();
  assert.deepEqual(events,[false]);assert.equal(f.closed,1);assert.equal(f.sources[0].disconnected,true);
});
test('加载期间停止后，迟到资源不会开始播放或发送回执',async()=>{
  const f=fixture(),load=deferred<typeof f.response>(),events:boolean[]=[];
  f.backend.fetch=async(_url,options)=>{assert.equal(options.signal.aborted,false);return load.promise;};
  const player=new TaskAudioPlayer(f.backend);player.play('slow',1000,v=>events.push(v));player.stop();
  load.resolve(f.response);await flush();
  assert.equal(f.sources.length,0);assert.equal(f.closed,1);assert.deepEqual(events,[]);
});
test('解码期间替换任务，旧解码不能复活音声或完成新任务',async()=>{
  const f=fixture(),decode=deferred<AudioBuffer>(),events:string[]=[];
  const create=f.backend.createContext;let count=0;
  f.backend.createContext=()=>{const context=create();if(++count===1)context.decodeAudioData=()=>decode.promise;return context;};
  const player=new TaskAudioPlayer(f.backend);player.play('old',1000,()=>events.push('old'));await flush();
  player.play('new',1000,()=>events.push('new'));await flush();decode.resolve({} as AudioBuffer);await flush();
  assert.equal(f.sources.length,1);assert.equal(f.sources[0].started,true);assert.deepEqual(events,[]);
  f.sources[0].onended!();assert.deepEqual(events,['new']);assert.equal(f.closed,2);
});
test('静音或取消立即停止输出，迟到 ended 不再报告正常完成',async()=>{
  const f=fixture(),events:boolean[]=[];const player=new TaskAudioPlayer(f.backend);
  player.play('voice',1000,v=>events.push(v));await flush();const late=f.sources[0].onended!;
  player.stop();late();await flush();
  assert.equal(f.sources[0].stopped,true);assert.equal(f.sources[0].disconnected,true);
  assert.equal(f.signal!.aborted,true);assert.equal(f.closed,1);assert.deepEqual(events,[]);
});
test('读取或解码失败只报告一次失败并清理资源',async()=>{
  for(const fault of ['http','decode','resume']){
    const f=fixture(),events:boolean[]=[];
    if(fault==='http')f.response.ok=false;
    const create=f.backend.createContext;
    f.backend.createContext=()=>{const c=create();if(fault==='decode')c.decodeAudioData=async()=>{throw Error('invalid');};if(fault==='resume')c.resume=async()=>{throw Error('device');};return c;};
    const player=new TaskAudioPlayer(f.backend);player.play('bad',1000,v=>events.push(v));await flush();
    assert.deepEqual(events,[true]);assert.equal(f.closed,1);assert.equal(f.sources.length,0);
  }
});
test('卡住的加载有界失败，超时后到达的资源不出声',async()=>{
  const f=fixture(),load=deferred<typeof f.response>(),events:boolean[]=[];
  f.backend.fetch=()=>load.promise;const player=new TaskAudioPlayer(f.backend);
  player.play('stalled',5,v=>events.push(v));await new Promise(r=>setTimeout(r,25));
  assert.deepEqual(events,[true]);load.resolve(f.response);await flush();
  assert.equal(f.sources.length,0);assert.equal(f.closed,1);assert.deepEqual(events,[true]);
});

test('整条音声预先解码，后段就绪前不出声，片段切换复用同一音频上下文',async()=>{
  const f=fixture(),last=deferred<typeof f.response>(),events:string[]=[];
  f.backend.fetch=async(url)=>url==='last'?last.promise:f.response;
  const player=new TaskAudioPlayer(f.backend);
  player.playSequence([{index:0,url:'first'},{index:1,url:'last'}],1000,v=>events.push(`done:${v}`),(i,e)=>events.push(`${i}:${e}`));
  await flush();assert.equal(f.sources.length,0);assert.deepEqual(events,[]);
  last.resolve(f.response);await flush();assert.equal(f.sources.length,1);assert.equal(f.closed,0);
  f.sources[0].onended!();assert.equal(f.sources.length,2);assert.equal(f.sources[1].started,true);assert.equal(f.closed,0);
  f.sources[1].onended!();assert.equal(f.closed,1);
  assert.deepEqual(events,['0:started','0:completed','1:started','1:completed','done:false']);
});

test('一段解码失败时其他片段顺序播放，整体明确报告降级',async()=>{
  const f=fixture(),events:string[]=[];
  f.backend.fetch=async(url)=>({ok:url!=='bad',arrayBuffer:async()=>new ArrayBuffer(8)});
  const player=new TaskAudioPlayer(f.backend);
  player.playSequence([{index:0,url:'bad'},{index:1,url:'good'}],1000,v=>events.push(`done:${v}`),(i,e)=>events.push(`${i}:${e}`));
  await flush();assert.equal(f.sources.length,1);f.sources[0].onended!();
  assert.deepEqual(events,['0:failed','1:started','1:completed','done:true']);
});

test('停止已预缓冲的序列，迟到 ended 不能接着播放下一段',async()=>{
  const f=fixture(),events:string[]=[];const player=new TaskAudioPlayer(f.backend);
  player.playSequence([{index:0,url:'first'},{index:1,url:'next'}],1000,v=>events.push(`done:${v}`),(i,e)=>events.push(`${i}:${e}`));
  await flush();const late=f.sources[0].onended!;player.stop();late();await flush();
  assert.equal(f.sources.length,1);assert.deepEqual(events,['0:started']);assert.equal(f.closed,1);
});
