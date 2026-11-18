import test from 'node:test';
import assert from 'node:assert/strict';
import {DecorationClock,DecorationFades,decorationDefaults,orderedDecorations,decorationVisible,decorationBounds,decorationSize,clipBounds,type Decoration} from '../packages/presentation/decorations.ts';
import type {Model,Clip} from '../packages/presentation/model.ts';
const clip:Clip={asset:'atlas',layout:{type:'grid',columns:8,rows:5,frameCount:40,cellWidth:199,cellHeight:71},frameDurationMs:200,direction:'forward'};
const model:Model={logicalSize:{width:500,height:500},anchors:{feet:{x:.5,y:.96},bubble:{x:.5,y:0}},clips:{border:clip,idle:{asset:'body',layout:{type:'single'},frameDurationMs:1000,direction:'forward'}},baseModes:{idle:{loop:'idle'},music:{loop:'idle'},quiet:{loop:'idle'}},semanticCues:{}};
const layer=(id='wreath',side:Decoration['side']='foreground')=>({...decorationDefaults(id,side),source:{type:'clip' as const,clip:'border'}});

test('背景和前景硬分组，80层无截断，同序保持作者顺序',()=>{
 const layers=Array.from({length:80},(_,i)=>({...layer(String(i),i%2?'background':'foreground'),order:-i}));
 const sorted=orderedDecorations(layers);assert.equal(sorted.length,80);assert.ok(sorted.slice(0,40).every(d=>d.side==='background'));assert.equal(sorted.at(-1)?.id,'0');
 assert.deepEqual(orderedDecorations([layer('a'),layer('b')]).map(d=>d.id),['a','b']);
});
test('按已应用状态匹配，多层并存，空状态不显示',()=>{
 const all=layer(),music={...layer(),states:['music'] as Decoration['states']};
 assert.equal(decorationVisible(all,'quiet'),true);assert.equal(decorationVisible(music,'idle'),false);assert.equal(decorationVisible(music,'music'),true);
 assert.equal(decorationVisible({...all,states:[]},'idle'),false);assert.equal(decorationVisible({...all,enabled:false},'music'),false);
});
test('独立速度、隐藏续播、排序定位变化不重置，换片段才重置',()=>{
 const a=layer('a'),b={...layer('b'),speed:2,states:['music'] as Decoration['states']};const clock=new DecorationClock();
 clock.sync([a,b],model,'idle',0);clock.tick(300);assert.equal(clock.frame('a',clip),1);assert.equal(clock.frame('b',clip),0);
 clock.sync([a,b],model,'music',300);clock.tick(600);assert.equal(clock.frame('a',clip),3);assert.equal(clock.frame('b',clip),3);
 clock.sync([a,b],model,'idle',600);clock.tick(1600);assert.equal(clock.elapsed('b'),600);
 clock.sync([a,b],model,'music',1600);clock.tick(1700);assert.equal(clock.elapsed('b'),800);
 clock.sync([{...a,order:100,offset:{x:100,y:-20}},{...b,speed:1}],model,'music',1700);clock.tick(1900);assert.equal(clock.elapsed('a'),1900);assert.equal(clock.elapsed('b'),1000);
 clock.sync([a,b],model,'music',1900,false);clock.tick(9000);assert.equal(clock.elapsed('a'),1900);
 clock.sync([a,b],model,'music',9000);clock.tick(9100);assert.equal(clock.elapsed('a'),2000);
 clock.sync([{...a,source:{type:'clip',clip:'idle'}},b],model,'music',9100);assert.equal(clock.elapsed('a'),0);assert.equal(clock.elapsed('b'),1200);
});
test('状态淡变约200毫秒；隐藏立即冻结帧，中途反向连续，重复同步不重置',()=>{
 const d={...layer(),states:['idle'] as Decoration['states']},fades=new DecorationFades(),clock=new DecorationClock();
 fades.sync([d],'idle',0);clock.sync([d],model,'idle',0);assert.equal(fades.opacity(d.id),1);
 fades.sync([d],'music',300);clock.sync([d],model,'music',300);
 fades.tick(400);clock.tick(400);assert.equal(fades.opacity(d.id),.5);assert.equal(clock.elapsed(d.id),300);
 fades.sync([d],'music',400);fades.tick(450);assert.equal(fades.opacity(d.id),.25);
 fades.sync([d],'idle',450);clock.sync([d],model,'idle',450);assert.equal(fades.opacity(d.id),.25);
 fades.tick(550);clock.tick(550);assert.equal(fades.opacity(d.id),.625);assert.equal(clock.elapsed(d.id),400);
 fades.tick(650);assert.equal(fades.opacity(d.id),1);
 fades.sync([{...d,enabled:false}],'idle',650);clock.sync([{...d,enabled:false}],model,'idle',650);
 assert.equal(fades.tick(849),false);assert.equal(fades.tick(850),true);assert.equal(fades.opacity(d.id),0);
 clock.tick(900);assert.equal(clock.elapsed(d.id),500);
 fades.sync([],'idle',900);fades.sync([d],'music',900);assert.equal(fades.opacity(d.id),0);
 fades.clear();fades.sync([d],'idle',950);assert.equal(fades.opacity(d.id),1);
});
test('花环大小独立，固定参照与视口裁切不改变模型比例',()=>{
 const d={...layer(),width:398,scale:.5};const size=decorationSize(d,model,{width:1592,height:355});assert.deepEqual(size,{width:199,height:71});
 assert.deepEqual(decorationBounds(d,model.logicalSize,size),{x:150.5,y:429,width:199,height:71});
 const over=decorationBounds({...d,offset:{x:-500,y:400}},model.logicalSize,size);assert.deepEqual(clipBounds(over,{x:-100,y:0,width:500,height:600}),{x:-100,y:829,width:0,height:0});
});

test('实际播放器同时画前后景；有限动作结束不重置常驻层',async t=>{
 const {SpriteRenderer}=await import('../packages/presentation/sprite.ts');let now=0,callback:()=>void=()=>{},drawn:string[]=[],alphas:{image:string;opacity:number}[]=[],resolveSlow:()=>void=()=>{};
 const keys=['document','Image','requestAnimationFrame','cancelAnimationFrame','devicePixelRatio'];const originals=new Map(keys.map(k=>[k,Object.getOwnPropertyDescriptor(globalThis,k)]));
 t.after(()=>{for(const k of keys){const d=originals.get(k);if(d)Object.defineProperty(globalThis,k,d);else delete (globalThis as any)[k];}});
 t.mock.method(performance,'now',()=>now);
 const savedAlpha:number[]=[];
 const ctx:any={globalAlpha:1,setTransform(){},clearRect(){},save(){savedAlpha.push(this.globalAlpha);},restore(){this.globalAlpha=savedAlpha.pop()??1;},beginPath(){},rect(){},clip(){},scale(){},drawImage(image:any){drawn.push(image.src);alphas.push({image:image.src,opacity:this.globalAlpha});},getImageData(_x:number,_y:number,w:number,h:number){return {data:new Uint8ClampedArray(w*h*4).fill(255)};}};
 const makeCanvas=()=>({width:0,height:0,dataset:{},getContext:()=>ctx});
 Object.assign(globalThis,{document:{hidden:false,createElement:()=>makeCanvas(),addEventListener(){},removeEventListener(){}},Image:class{width=199;height=71;src='';async decode(){if(this.src==='slow')await new Promise<void>(resolve=>resolveSlow=resolve);}},devicePixelRatio:1,requestAnimationFrame:(cb:()=>void)=>{callback=cb;return 1;},cancelAnimationFrame(){}});
 const canvas=makeCanvas(),r=new SpriteRenderer(canvas as any);
 const front:Decoration={...layer('front'),source:{type:'image',asset:'front'}},back:Decoration={...layer('back','background'),source:{type:'image',asset:'back'},offset:{x:-300,y:0}};
 await r.load({...model,decorations:[front,back]}, {body:'body',atlas:'atlas',front:'front',back:'back',new:'new',slow:'slow'});
 drawn=[];callback();assert.deepEqual(drawn,['back','body','front']);
 let finished=0;r.playFinite(['border'],1,()=>finished++);drawn=[];now=250;callback();assert.deepEqual(drawn.slice(-3),['back','atlas','front']);
 now=8500;callback();await Promise.resolve();assert.equal(finished,1);assert.equal(canvas.dataset['presentation' as keyof typeof canvas.dataset],'base');
 r.cancel();drawn=[];callback();assert.deepEqual(drawn,['back','body','front']);
 // Fading a layer outside the model keeps its canvas extent until it disappears.
 const expanded=r.sceneBounds();assert.ok(expanded.x<0);
 r.setDecorations([front,{...back,enabled:false}]);assert.deepEqual(r.sceneBounds(),expanded);
 now=8600;alphas=[];callback();assert.deepEqual(alphas,[{image:'back',opacity:.5},{image:'body',opacity:1},{image:'front',opacity:1}]);assert.deepEqual(r.sceneBounds(),expanded);
 r.setDecorations([front,back]);alphas=[];callback();assert.equal(alphas[0].opacity,.5);
 now=8700;alphas=[];callback();assert.equal(alphas[0].opacity,.75);
 now=8800;callback();r.setDecorations([front,{...back,enabled:false}]);
 now=8900;callback();assert.deepEqual(r.sceneBounds(),expanded);
 now=9000;drawn=[];callback();assert.deepEqual(drawn,['body','front']);assert.deepEqual(r.sceneBounds(),{x:0,y:0,...model.logicalSize});
 r.setDecorations([{...front,states:['music']}]);now=9100;alphas=[];callback();assert.equal(alphas.at(-1)?.opacity,.5);
 r.applyBase('music');alphas=[];callback();assert.equal(alphas.at(-1)?.opacity,.5);
 r.applyBase('music');now=9200;alphas=[];callback();assert.deepEqual(alphas,[{image:'body',opacity:1},{image:'front',opacity:.75}]);
 now=9300;callback();
 // A newly selected image is loaded without restarting the body or whole scene.
 await r.updateDecorations([{...layer('front'),source:{type:'image',asset:'new'}}]);
 drawn=[];callback();assert.deepEqual(drawn,['body','new']);
 const late=r.updateDecorations([{...layer('front'),source:{type:'image',asset:'slow'}}]);
 await r.updateDecorations([{...layer('front'),source:{type:'image',asset:'front'}}]);
 resolveSlow();await late;drawn=[];callback();assert.deepEqual(drawn,['body','front']);
 const stale=r.updateDecorations([{...layer('front'),source:{type:'image',asset:'new'}}]);
 await r.load(model,{body:'body',atlas:'atlas'});await stale;
 drawn=[];callback();assert.deepEqual(drawn,['body']);
 // The first frame uses the host's applied state, without briefly showing idle layers.
 drawn=[];alphas=[];
 await r.load({...model,decorations:[{...front,states:['music']},{...back,states:['idle']}]},{body:'body',atlas:'atlas',front:'front',back:'back'},'music');
 assert.equal(canvas.dataset['base' as keyof typeof canvas.dataset],'music');assert.ok(!drawn.includes('back'));
 assert.deepEqual(alphas.slice(-2),[{image:'body',opacity:1},{image:'front',opacity:1}]);
 r.applyBase('music');now+=100;drawn=[];alphas=[];callback();assert.deepEqual(drawn,['body','front']);assert.equal(alphas.at(-1)?.opacity,1);
 r.dispose();drawn=[];callback();assert.deepEqual(drawn,[]);
});
