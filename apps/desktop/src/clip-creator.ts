import {clipChecks,inferGrid,optionalFlags,parseRects,suggestedClipId,type ImageSize} from '../../../packages/editor/clip-authoring.ts';
import {sourceRect} from '../../../packages/presentation/decorations.ts';
import {frameOrder,timeline,type Clip} from '../../../packages/presentation/model.ts';

type Options={assets:Record<string,{kind:string}>;paths:Record<string,string>;clips:Record<string,Clip>;assetUrl:(path:string)=>string;check:(clip:Clip)=>Promise<unknown>;created:(id:string,clip:Clip)=>void};
export function openClipCreator(o:Options){
  const dialog=document.createElement('dialog');dialog.className='clip-creator';dialog.setAttribute('aria-labelledby','clip-creator-title');
  dialog.innerHTML=`<form novalidate><div class="section-title"><h2 id="clip-creator-title">添加动作片段</h2><button type="button" data-close aria-label="关闭添加片段窗口">关闭</button></div>
  <p>从素材库选择图片，静态片段无需动画参数。创建后加入当前项目草稿，再保存并使用工作台预览。</p>
  <div class="grid-two"><label>片段类型<select name="kind"><option value="single">静态图片</option><option value="sequence">序列帧动画</option></select></label><label>图片素材<select name="asset"><option value="">请选择素材库图片…</option></select></label><label class="clip-wide">片段编号（自动生成，可修改）<input name="id" maxlength="256" autocomplete="off"></label></div>
  <p data-image-info role="status">请先选择图片。</p>
  <section data-sequence hidden><label>切帧布局 layout.type<select name="layout"><option value="grid">规则网格（grid）</option><option value="rects">逐帧矩形（rects）</option></select></label>
    <div data-grid><div class="grid-two">
    <label>列数 columns<input name="columns" type="number" min="1" max="4096" step="1"></label><label>行数 rows<input name="rows" type="number" min="1" max="4096" step="1"></label>
    <label>有效帧数 frameCount<input name="frameCount" type="number" min="1" max="4096" step="1"></label><label>单帧宽度 cellWidth（像素）<input name="cellWidth" type="number" min="1" max="16384" step="1"></label><label>单帧高度 cellHeight（像素）<input name="cellHeight" type="number" min="1" max="16384" step="1"></label></div>
    <div class="actions"><button type="button" data-infer="counts">按行列推算格宽、高及帧数</button><button type="button" data-infer="cells">按单帧宽高推算行列及帧数</button></div>
    <p class="hint">先填写其中一组再推算。推算只根据图片尺寸计算，不识别美术帧；右侧或底部余量会提示。末行未满时可自行减少有效帧数。</p></div>
    <label data-rects hidden>每行一帧：x, y, width, height<textarea name="rects" rows="5" maxlength="200000" placeholder="0, 0, 199, 71&#10;199, 0, 199, 71"></textarea><small>坐标以原图片左上角为原点；可有不等大小的帧、留白或重复帧。</small></label>
    <div class="grid-two"><label>每帧时长 frameDurationMs（毫秒）<input name="frameDurationMs" type="number" min="16" max="60000" value="200"></label><label>播放方向 direction<select name="direction"><option value="forward">正向（forward）</option><option value="reverse">反向（reverse）</option><option value="ping-pong">往返（ping-pong）</option></select></label></div>
    <details><summary>往返播放选项（仅 ping-pong 生效）</summary><div class="grid-two" data-flags></div><p class="hint">留空/null 表示未指定，等效不启用。保存时省略未指定字段；显式 false 会保留。</p></details>
  </section>
  <div class="clip-image-preview"><div><h3>图片与切片边界</h3><canvas data-atlas></canvas></div><div><h3>选中帧</h3><canvas data-frame></canvas><label>检查源帧<input name="frame" type="range" min="1" max="1" value="1"><span data-frame-label>—</span></label></div></div>
  <p data-playback class="hint"></p><p data-feedback role="status"></p><p>参数可绘制不代表切帧正确。请检查每格是否包含完整图案；创建后通过“预览所选片段”核对连贯性与节奏。</p>
  <div class="actions"><button type="button" data-check>检查参数</button><button type="submit" class="primary" data-create>添加到动画片段</button><button type="button" data-close>取消</button></div></form>`;
  const $=<T extends HTMLElement=HTMLElement>(s:string)=>dialog.querySelector<T>(s)!;
  const input=(name:string)=>$<HTMLInputElement>(`[name="${name}"]`),value=(name:string)=>input(name).value,num=(name:string)=>Number(value(name));
  let image:HTMLImageElement|null=null,size:ImageSize|null=null,loading=0,checking=false,closed=false,idEdited=false,editVersion=0;
  for(const [id,a] of Object.entries(o.assets))if(a.kind==='image'){const option=document.createElement('option');option.value=id;option.textContent=id;$('[name=asset]').append(option);}
  for(const [key,title]of [['repeatEndpoints','往返两端重复一帧'],['returnToStart','往返结束回到起点'],['startReverse','反向起步']]as const){const l=document.createElement('label');l.textContent=`${title} ${key}`;const select=document.createElement('select');select.name=key;for(const [v,text]of [['','未指定（null / 不启用）'],['true','启用（true）'],['false','关闭（false）']]){const option=document.createElement('option');option.value=v;option.textContent=text;select.append(option);}l.append(select);$('[data-flags]').append(l);}
  const feedback=(text:string,error=false)=>{const p=$('[data-feedback]');p.textContent=text;p.classList.toggle('error',error);};
  function clip():Clip {
    if(value('kind')==='single')return {asset:value('asset'),layout:{type:'single'},frameDurationMs:1000,direction:'forward'};
    const layout:Clip['layout']=value('layout')==='grid'?{type:'grid',columns:num('columns'),rows:num('rows'),frameCount:num('frameCount'),cellWidth:num('cellWidth'),cellHeight:num('cellHeight')}:{type:'rects',frames:parseRects(value('rects'))};
    return {asset:value('asset'),layout,frameDurationMs:num('frameDurationMs'),direction:value('direction') as Clip['direction'],...optionalFlags({repeatEndpoints:value('repeatEndpoints'),returnToStart:value('returnToStart'),startReverse:value('startReverse')})};
  }
  function inspect(){
    const id=value('id').trim();if(!id||id.length>256)throw Error('请填写 1—256 个字符的片段编号。');
    if(Object.hasOwn(o.clips,id))throw Error('片段编号已存在，请换一个编号；不会覆盖已有片段。');
    if(!size||!image)throw Error('请先选择并成功读取一张素材图片。');
    const c=clip(),result=clipChecks(c,size);if(result.errors.length)throw Error(result.errors.join('\n'));
    return {id,c,warnings:result.warnings};
  }
  function draw(){
    const sequence=value('kind')==='sequence';$('[data-sequence]').hidden=!sequence;$('[data-grid]').hidden=value('layout')!=='grid';$('[data-rects]').hidden=value('layout')!=='rects';
    const atlas=$<HTMLCanvasElement>('[data-atlas]'),frame=$<HTMLCanvasElement>('[data-frame]');
    for(const c of [atlas,frame]){c.width=480;c.height=260;c.getContext('2d')!.clearRect(0,0,c.width,c.height);}
    $('[data-playback]').textContent='';if(!image||!size)return;
    const scale=Math.min(480/size.width,260/size.height),x=(480-size.width*scale)/2,y=(260-size.height*scale)/2,ctx=atlas.getContext('2d')!;
    ctx.drawImage(image,x,y,size.width*scale,size.height*scale);
    try{
      const c=clip(),check=clipChecks(c,size);if(check.errors.length){feedback(check.errors.join('\n'),true);return;}
      const count=c.layout.type==='single'?1:c.layout.type==='grid'?c.layout.frameCount!:c.layout.frames!.length;
      input('frame').max=String(count);const index=Math.max(0,Math.min(count-1,num('frame')-1));input('frame').value=String(index+1);input('frame').disabled=count===1;
      for(let i=0;i<count;i++){const r=sourceRect(c,size,i);ctx.strokeStyle=i===index?'#d54d15':'#75516e';ctx.lineWidth=i===index?2:1;ctx.strokeRect(x+r.x*scale,y+r.y*scale,r.width*scale,r.height*scale);}
      const r=sourceRect(c,size,index),s=Math.min(480/r.width,260/r.height);frame.getContext('2d')!.drawImage(image,r.x,r.y,r.width,r.height,(480-r.width*s)/2,(260-r.height*s)/2,r.width*s,r.height*s);
      $('[data-frame-label]').textContent=`第 ${index+1} / ${count} 帧 · ${r.width}×${r.height} 像素`;
      const order=frameOrder(c);$('[data-playback]').textContent=sequence?`单轮 ${order.length} 次帧显示，${(timeline(c).at(-1)!.end/1000).toFixed(2)} 秒。帧序：${order.slice(0,20).map(n=>n+1).join(' → ')}${order.length>20?' …':''}`:'静态片段使用完整图片，不需要填写帧数或时间。';
      feedback(check.warnings.join('\n')||'尺寸与参数检查通过；切帧内容仍请对照预览。');
    }catch(e){feedback(e instanceof Error?e.message:String(e),true);}
  }
  async function loadImage(){
    const epoch=++loading;image=null;size=null;draw();
    if(!idEdited)input('id').value=suggestedClipId(value('asset'),Object.keys(o.clips));
    const path=o.paths[value('asset')];if(!path){$('[data-image-info]').textContent='未找到图片，请先到素材库导入。';return;}
    $('[data-image-info]').textContent='正在读取图片尺寸…';
    const im=new Image();im.crossOrigin='anonymous';im.src=o.assetUrl(path);
    try{await im.decode();if(closed||epoch!==loading)return;image=im;size={width:im.naturalWidth,height:im.naturalHeight};$('[data-image-info]').textContent=`原图 ${size.width} × ${size.height} 像素。`;draw();}
    catch{if(!closed&&epoch===loading){$('[data-image-info]').textContent='图片无法读取，请检查素材。';feedback('图片无法解码，不能创建片段。',true);}}
  }
  const close=()=>{closed=true;++loading;dialog.close();dialog.remove();};dialog.querySelectorAll('[data-close]').forEach(b=>b.addEventListener('click',close));dialog.addEventListener('cancel',e=>{e.preventDefault();close();});
  dialog.addEventListener('input',()=>{editVersion++;});dialog.addEventListener('change',()=>{editVersion++;});
  input('id').addEventListener('input',()=>{idEdited=true;});input('asset').addEventListener('change',()=>void loadImage());
  dialog.addEventListener('input',e=>{if((e.target as HTMLElement).matches('[name]:not([name=asset]):not([name=id])'))draw();});
  dialog.addEventListener('change',e=>{if((e.target as HTMLElement).matches('select:not([name=asset])'))draw();});
  dialog.querySelectorAll<HTMLButtonElement>('[data-infer]').forEach(b=>b.onclick=()=>{try{if(!size)throw Error('请先选择图片。');const by=b.dataset.infer as 'counts'|'cells',g=inferGrid(size,by,num(by==='counts'?'columns':'cellWidth'),num(by==='counts'?'rows':'cellHeight'));for(const [key,n]of Object.entries(g))input(key).value=String(n);editVersion++;draw();}catch(e){feedback(String(e),true);}});
  async function check(create:boolean){if(checking)return;checking=true;$<HTMLButtonElement>('[data-create]').disabled=true;$<HTMLButtonElement>('[data-check]').disabled=true;
    try{const checked=inspect(),version=editVersion;await o.check(checked.c);if(closed)return;if(version!==editVersion)throw Error('校验期间参数已改变，请重新检查或添加。');if(create){o.created(checked.id,checked.c);close();}else feedback(['宿主校验通过：图片可用、参数合法且切片未越界。',...checked.warnings,'这不保证帧内容与节奏符合预期，请继续预览。'].join('\n'));}
    catch(e){if(!closed)feedback(e instanceof Error?e.message:String(e),true);}finally{checking=false;if(!closed){$<HTMLButtonElement>('[data-create]').disabled=false;$<HTMLButtonElement>('[data-check]').disabled=false;}}
  }
  $('form').addEventListener('submit',e=>{e.preventDefault();void check(true);});$('[data-check]').onclick=()=>void check(false);
  document.body.append(dialog);dialog.showModal();draw();input('asset').focus();
  return close;
}
