import {setupControl} from '../src/control.ts';
import {defaultBubblePreferences} from '../../../packages/presentation/bubble.ts';
import plugin from '../../../plugins/builtin/pointer-interaction.json';
import type {Snapshot} from '../src/types.ts';
import '../src/style.css';
if(!import.meta.env.DEV)throw Error('Development-only synthetic preview');
const config={...plugin.manifest.defaults,response:{mode:'fixed',variants:[{id:'sequence',text:['我在这里。','你好，今天过得怎么样？'],audioAsset:'voice.evening,voice.morning\nvoice.noon',cue:'greeting\ngreeting',readMs:1000}]}};
setupControl(document.querySelector<HTMLElement>('#app')!)({version:'0.3.0',stage:'synthetic form preview',instance:{id:'preview',name:'预览伙伴'},instances:[{id:'preview',name:'预览伙伴'}],package:null,manifest:null,model:null,assetPaths:{},library:[],plugins:[{id:plugin.manifest.pluginId,name:plugin.manifest.displayName,schema:plugin.schema}],configured:[{owner:'example-sequence',pluginId:plugin.manifest.pluginId,name:'点击互动 · 多素材示例',enabled:false,priority:'normal',status:'仅预览',configVersion:'1.0',config}],unknownPresets:[],facts:{fullscreen:false,media:false,mediaSources:[],fullscreenStatus:'合成资料',mediaStatus:'合成资料'},zoom:1,bubble:defaultBubblePreferences,excluded:[],dataDirectory:'仅预览',scheduler:{active:null,tasks:[],desiredBase:'idle',appliedBase:'idle',suppressed:false,muted:false,log:[],claims:[]}} as unknown as Snapshot);
