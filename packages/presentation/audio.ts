export interface AudioBackend {
  createContext: () => AudioContext;
  fetch: (url: string, options: {signal: AbortSignal}) => Promise<{ok: boolean; arrayBuffer: () => Promise<ArrayBuffer>}>;
}
interface Playback {
  controller: AbortController;
  context: AudioContext | null;
  source: AudioBufferSourceNode | null;
  timer: ReturnType<typeof setTimeout>;
  done: (failed: boolean) => void;
  segment?: (index:number,event:'started'|'completed'|'failed') => void;
  failed: boolean;
}

/** Companion voice clips use Web Audio without registering a media session.
 * Windows output volume/device selection still applies. Completion is an actual
 * source-ended callback, independent of text, animation and environment facts. */
export class TaskAudioPlayer {
  private current: Playback | null = null;
  private backend: AudioBackend;
  constructor(backend: AudioBackend = {
    createContext: () => new AudioContext({latencyHint: 'playback'}),
    fetch: (url, options) => fetch(url, options),
  }) { this.backend = backend; }

  play(url: string, deadlineMs: number, done: (failed: boolean) => void, started?: () => void) {
    this.playSequence([{index:0,url}],deadlineMs,done,(_index,event)=>{if(event==='started')started?.();});
  }
  playSequence(parts:{index:number;url:string}[],deadlineMs:number,done:(failed:boolean)=>void,segment?:Playback['segment']) {
    this.stop();
    const playback:Playback={controller:new AbortController(),context:null,source:null,
      timer:setTimeout(()=>this.finish(playback,true),deadlineMs),done,segment,failed:false};
    this.current=playback;
    void this.prepare(playback,parts);
  }
  stop() {
    const playback = this.current;
    this.current = null;
    if (playback) this.release(playback);
  }
  private async prepare(playback: Playback, parts:{index:number;url:string}[]) {
    try {
      const context=this.backend.createContext();playback.context=context;
      await context.resume();
      if(this.current!==playback)return;
      const buffers:(AudioBuffer|null)[]=new Array(parts.length).fill(null);
      let next=0,decodedBytes=0,duration=0;
      const load=async()=>{
        while(next<parts.length&&this.current===playback){
          const slot=next++;
          try{
            const response=await this.backend.fetch(parts[slot].url,{signal:playback.controller.signal});
            if(this.current!==playback)return;
            if(!response.ok)throw Error('音声读取失败');
            const bytes=await response.arrayBuffer();
            if(this.current!==playback)return;
            const buffer=await context.decodeAudioData(bytes);
            if(this.current!==playback)return;
            const size=buffer.length*buffer.numberOfChannels*4;
            if(decodedBytes+size>128*1024*1024||duration+buffer.duration>600)throw Error('音声解码缓冲达到上限');
            decodedBytes+=size;duration+=buffer.duration;buffers[slot]=buffer;
          }catch{
            if(this.current!==playback)return;
            playback.failed=true;playback.segment?.(parts[slot].index,'failed');
          }
        }
      };
      // Bound fetch/decode concurrency; nothing starts until every slot has settled.
      await Promise.all([load(),load()]);
      if(this.current!==playback)return;
      const start=(slot:number):void=>{
        if(this.current!==playback)return;
        while(slot<parts.length&&!buffers[slot])slot++;
        if(slot>=parts.length){this.finish(playback,playback.failed);return;}
        const source=context.createBufferSource();playback.source=source;source.buffer=buffers[slot];
        source.connect(context.destination);
        source.onended=()=>{
          if(this.current!==playback)return;
          source.onended=null;source.disconnect();playback.source=null;buffers[slot]=null;
          playback.segment?.(parts[slot].index,'completed');
          start(slot+1);
        };
        try{source.start();playback.segment?.(parts[slot].index,'started');}
        catch{source.onended=null;source.disconnect();playback.source=null;playback.failed=true;playback.segment?.(parts[slot].index,'failed');start(slot+1);}
      };
      start(0);
    } catch {this.finish(playback,true);}
  }
  private finish(playback: Playback, failed: boolean) {
    if (this.current !== playback) return;
    this.current = null;
    this.release(playback);
    playback.done(failed);
  }
  private release(playback: Playback) {
    clearTimeout(playback.timer);
    playback.controller.abort();
    if (playback.source) {
      playback.source.onended = null;
      try { playback.source.stop(); } catch { /* Already ended or not yet started. */ }
      playback.source.disconnect();
      playback.source = null;
    }
    if (playback.context) {
      void playback.context.close().catch(() => {});
      playback.context = null;
    }
  }
}
