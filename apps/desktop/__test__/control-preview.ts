import {setupControl} from '../src/control.ts';
import {defaultBubblePreferences} from '../../../packages/presentation/bubble.ts';
import '../src/style.css';
import type {Snapshot} from '../src/types.ts';
if(!import.meta.env.DEV)throw Error('Development-only preview');
const fixture=await fetch('./local-fixture.json').then(r=>r.json());
setupControl(document.querySelector('#app')!)({...fixture,version:'0.1.0',stage:'candidate 2',instance:{id:'preview-001',name:'樱色 Ema'},instances:[{id:'preview-001',name:'樱色 Ema'}],package:{hash:'preview',id:'local.ema.prototype',version:'0.1.0'},library:[],plugins:[],configured:[],unknownPresets:[],facts:{fullscreen:false,media:false,mediaSources:[],fullscreenStatus:'桌宠所在显示器',mediaStatus:'Windows 媒体会话'},zoom:1,bubble:defaultBubblePreferences,excluded:[],dataDirectory:'本机独立数据目录',scheduler:{active:null,tasks:[],desiredBase:'idle',appliedBase:'idle',suppressed:false,muted:false,log:[],claims:[]}} as Snapshot);
