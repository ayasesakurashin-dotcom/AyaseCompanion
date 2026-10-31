export interface BubblePreferences {
  animation: 'typewriter' | 'none';
  charactersPerSecond: number;
  fontSize: number;
  readMs: number;
}
export const defaultBubblePreferences: BubblePreferences = {
  animation: 'typewriter', charactersPerSecond: 30, fontSize: 15, readMs: 2800,
};
export interface Bounds { x: number; y: number; width: number; height: number }
export function bubbleGeometry(reference: Bounds, viewport: {width:number;height:number}, zoom:number) {
  const gap = 12 * zoom;
  const bottom = viewport.height - reference.y + gap;
  const width = Math.max(48, Math.min(viewport.width - 24, Math.max(148, reference.width * 1.65)));
  return {
    width,
    maxHeight: Math.max(0, Math.min(reference.height, reference.y - gap - 12)),
    bottom,
    center: Math.max(width / 2 + 12, Math.min(viewport.width - width / 2 - 12, reference.x + reference.width / 2)),
  };
}
export function graphemes(text:string):string[] {
  return Array.from(new Intl.Segmenter(undefined, {granularity:'grapheme'}).segment(text), s=>s.segment);
}
/** The callback measures with the same browser font, width and wrapping as the live bubble. */
export function measuredPages(chars:readonly string[], fits:(text:string)=>boolean):string[] {
  const pages:string[]=[];
  for(let start=0;start<chars.length;){
    let low=1,high=chars.length-start,best=0;
    while(low<=high){const count=Math.floor((low+high)/2);if(fits(chars.slice(start,start+count).join(''))){best=count;low=count+1;}else high=count-1;}
    // A viewport too small for one line is handled by the presenter, never by dropping text.
    const count=Math.max(1,best);pages.push(chars.slice(start,start+count).join(''));start+=count;
  }
  return pages;
}
export function revealCount(elapsed:number,count:number,preferences:BubblePreferences):number {
  return preferences.animation==='none'?count:Math.min(count,Math.floor(Math.max(0,elapsed)*preferences.charactersPerSecond/1000));
}
export function readingTime(count:number,preferences:BubblePreferences,taskReadMs?:number):number {
  return Math.max(taskReadMs??preferences.readMs,count*70);
}

/** Text owns its own completion and visibility; audio/animation do not extend its lifetime. */
export class BubblePresenter {
  private text:HTMLElement;
  private counter:HTMLElement;
  private measure:HTMLElement;
  private timer=0;
  private generation=0;
  private preferences=defaultBubblePreferences;
  private all:string[]=[];
  private pages:string[]=[];
  private pageStart=0;
  private revealed=0;
  private taskReadMs:number|undefined;
  private done:((failed?:boolean)=>void)|null=null;
  private signature='';
  private maxTextHeight=0;
  private phase:'off'|'revealing'|'reading'|'closing'='off';
  private streaming=false;
  private waiting=false;
  private progress:((text:string)=>void)|null=null;
  private remaining:string[]=[];
  private completedText='';

  private element:HTMLElement;
  constructor(element:HTMLElement){
    this.element=element;
    element.innerHTML='<p class="bubble-text"></p><small class="bubble-counter"></small><p class="bubble-measure" aria-hidden="true"></p>';
    this.text=element.querySelector('.bubble-text')!;
    this.counter=element.querySelector('.bubble-counter')!;
    this.measure=element.querySelector('.bubble-measure')!;
  }
  layout(reference:Bounds,viewport:{width:number;height:number},zoom:number,preferences:BubblePreferences){
    const geometry=bubbleGeometry(reference,viewport,zoom);
    const signature=JSON.stringify([geometry,preferences]);
    if(signature===this.signature)return;
    this.signature=signature;this.preferences={...preferences};
    this.element.style.width=`${geometry.width}px`;
    this.element.style.left=`${geometry.center}px`;
    this.element.style.bottom=`${geometry.bottom}px`;
    this.element.style.maxHeight=`${geometry.maxHeight}px`;
    this.element.style.setProperty('--bubble-font-size',`${preferences.fontSize}px`);
    this.maxTextHeight=geometry.maxHeight-44;
    if(this.done&&!this.waiting&&this.phase!=='closing')this.showPage(true);
  }
  start(text:string,readMs:number|undefined,done:(failed?:boolean)=>void,progress?:((text:string)=>void)){
    this.startSequence([text],readMs,done,progress);
  }
  startSequence(texts:string[],readMs:number|undefined,done:(failed?:boolean)=>void,progress?:((text:string)=>void)){
    this.cancel();this.remaining=texts.map(text=>text.replace(/\r\n/g,'\n')).filter(text=>text.trim());
    this.all=graphemes(this.remaining.shift()??'');this.taskReadMs=readMs;this.done=done;
    this.progress=progress??null;
    this.showPage(false);
  }
  startStream(text:string,readMs:number|undefined,done:(failed?:boolean)=>void,progress:(text:string)=>void){
    this.cancel();this.streaming=true;this.taskReadMs=readMs;this.done=done;this.progress=progress;
    this.all=graphemes(text.replace(/\r\n/g,'\n'));if(this.all.length)this.showPage(false);else this.waiting=true;
  }
  append(text:string,ended:boolean){
    if(!this.done)return;
    const normalized=text.replace(/\r\n/g,'\n');if(!normalized.startsWith(this.all.join('')))return;
    this.all=graphemes(normalized);this.streaming=!ended;
    if(this.waiting){if(this.pageStart<this.all.length){this.waiting=false;this.showPage(false);}else if(ended)this.finish();}
  }
  cancel(){
    ++this.generation;clearTimeout(this.timer);this.done=null;this.phase='off';this.all=[];this.pageStart=0;this.revealed=0;this.streaming=false;this.waiting=false;this.progress=null;this.remaining=[];this.completedText='';
    this.element.classList.add('hidden');this.element.classList.remove('closing');this.element.dataset.phase='off';
    this.text.textContent='';this.counter.textContent='';
  }
  private later(fn:()=>void,ms:number){
    const generation=this.generation;
    this.timer=window.setTimeout(()=>{if(generation===this.generation)fn();},ms);
  }
  private showPage(reflow:boolean){
    clearTimeout(this.timer);++this.generation;
    this.element.classList.remove('hidden','closing');
    this.element.style.height='auto';
    // An impossibly small viewport is a channel failure, never an indefinite task lock.
    if(this.maxTextHeight<this.preferences.fontSize*1.65){
      const done=this.done;this.cancel();console.warn('气泡空间不足，请缩小模型或字号');done?.(true);return;
    }
    this.pages=measuredPages(this.all.slice(this.pageStart),text=>{
      this.measure.textContent=text;
      return this.measure.getBoundingClientRect().height<=this.maxTextHeight+0.1;
    });
    if(!this.pages.length){this.finish();return;}
    const chars=graphemes(this.pages[0]);
    const already=reflow?Math.min(chars.length,this.revealed):0;
    this.revealed=already;
    this.measure.textContent=this.pages[0];
    // Reserve this page's final height during reveal, so the lower edge and the text do not jump.
    this.text.style.height=`${this.measure.getBoundingClientRect().height}px`;
    this.counter.textContent=this.pages.length>1?`后续 ${this.pages.length-1} 页`:'';
    this.text.textContent=chars.slice(0,already).join('');
    this.phase='revealing';this.element.dataset.phase='revealing';
    const start=performance.now();
    const reveal=()=>{
      this.revealed=Math.min(chars.length,already+revealCount(performance.now()-start,chars.length,this.preferences));
      this.text.textContent=chars.slice(0,this.revealed).join('');
      this.progress?.(this.completedText+this.all.slice(0,this.pageStart+this.revealed).join(''));
      if(this.revealed<chars.length){this.later(reveal,25);return;}
      this.phase='reading';this.element.dataset.phase='reading';
      this.later(()=>{
        this.pageStart+=chars.length;this.revealed=0;
        if(this.pageStart<this.all.length)this.showPage(false);else if(this.streaming){this.waiting=true;this.phase='reading';}else this.finish();
      },readingTime(chars.length,this.preferences,this.taskReadMs));
    };
    reveal();
  }
  private finish(){
    if(this.remaining.length){
      this.completedText+=this.all.join('')+'\n';this.all=graphemes(this.remaining.shift()!);this.pageStart=0;this.revealed=0;
      this.showPage(false);return;
    }
    this.phase='closing';this.element.dataset.phase='closing';this.element.classList.add('closing');
    this.later(()=>{const done=this.done;this.cancel();done?.();},180);
  }
}
