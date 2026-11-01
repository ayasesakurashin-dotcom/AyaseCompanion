import {setupControl} from '../src/control.ts';
import {defaultBubblePreferences} from '../../../packages/presentation/bubble.ts';
import plugin from '../../../plugins/builtin/pointer-interaction.json';
import type {Snapshot} from '../src/types.ts';
import '../src/style.css';
if(!import.meta.env.DEV)throw Error('Development-only synthetic preview');
const config={...plugin.manifest.defaults,response:{mode:'state-specific',stateResponses:[
  {id:'idle-route',base:'idle',response:{mode:'fixed',variants:[{id:'A',text:['你好。','我在这里。'],animationAsset:'anime.music_start\nidle',readMs:1000},{id:'B',text:'今天怎么样？'}]}},
  {id:'music-route',base:'music',response:{mode:'generated-with-fallback',intent:'用户正在听音乐，回应用户的点击。',animationAsset:['anime.music_start','idle'],fallback:[{id:'C',text:'这首歌听起来很不错。'},{id:'D',text:'陪你听一会儿。'}]}}
]}};
const clip={asset:'visual.idle',layout:{type:'single'},frameDurationMs:1000,direction:'forward'};
setupControl(document.querySelector<HTMLElement>('#app')!)({version:'0.3.0',stage:'synthetic form preview',instance:{id:'preview',name:'预览伙伴'},instances:[{id:'preview',name:'预览伙伴'}],package:null,manifest:null,model:{logicalSize:{width:64,height:64},anchors:{feet:{x:.5,y:1},bubble:{x:.5,y:0}},clips:{idle:clip,'anime.music_start':{...clip,asset:'visual.music-start'},'anime.music_end':{...clip,asset:'visual.music-end'}},baseModes:{idle:{loop:'idle'},music:{loop:'idle'},quiet:{loop:'idle'}},semanticCues:{}},assetPaths:{},library:[],plugins:[{id:plugin.manifest.pluginId,name:plugin.manifest.displayName,schema:plugin.schema}],configured:[{owner:'state-example',pluginId:plugin.manifest.pluginId,name:'点击互动 · 状态回复示例',enabled:false,priority:'normal',status:'仅预览',configVersion:'1.0',config}],unknownPresets:[],facts:{fullscreen:false,media:false,mediaSources:[],fullscreenStatus:'合成资料',mediaStatus:'合成资料'},zoom:1,bubble:defaultBubblePreferences,excluded:[],dataDirectory:'仅预览',scheduler:{active:null,tasks:[],desiredBase:'idle',appliedBase:'idle',suppressed:false,muted:false,log:[],claims:[]}} as unknown as Snapshot);
