import fs from 'node:fs';
import path from 'node:path';
const write=(file,value)=>{fs.mkdirSync(path.dirname(file),{recursive:true});fs.writeFileSync(file,JSON.stringify(value,null,2)+'\n');};
const time={type:'string',pattern:'^([01][0-9]|2[0-3]):[0-5][0-9]$'};
const ttl={type:'integer',minimum:1,maximum:3600,default:300,title:'排队有效期（秒）'};
const response=JSON.parse(fs.readFileSync('contracts/request.schema.json','utf8'));
const manifest=(id,name,caps,defaults)=>({format:'ayase.plugin',formatVersion:'1.0',pluginId:id,version:'0.1.0',displayName:name,apiVersion:'1.0',configVersion:'1.0',runtime:{type:'declarative.v1',entry:'rules/main.json'},configSchema:'schemas/config.schema.json',defaultPriority:'normal',capabilities:caps,defaults});
const present={type:'presentation.request',responseFromConfig:'/response',ttlSecondsFromConfig:'/ttlSeconds'};
const wrap=(manifest,schema,subscriptions,rules)=>({manifest,schema,rules:{schemaVersion:'1.0',subscriptions,rules}});
const allDay={id:'all-day',start:'00:00',end:'00:00',allDay:true,response:{mode:'fixed',variants:[{id:'hello',text:'你好，我在这里。',cue:'greeting'}]}};
write('plugins/builtin/startup.json',wrap(
  manifest('ayase.startup-greeting','启动问候',['presentation.request'],{source:'app.ready',timeZone:'system',ttlSeconds:300,windows:[allDay]}),
  {type:'object',required:['source','timeZone','ttlSeconds','windows'],properties:{source:{const:'app.ready',title:'事件来源'},timeZone:{type:'string',title:'时区'},ttlSeconds:ttl,windows:{type:'array',maxItems:24,items:{type:'object',required:['id','start','end','response'],properties:{id:{type:'string'},start:time,end:time,allDay:{type:'boolean'},response}}}}},
  [{id:'ready',type:'app.ready'}],[{id:'greet',on:'ready',actions:[{...present,responseFromConfig:'/windows'}]}]
));
for(const [short,id,name,event,mode,suppress,priority] of [
  ['fullscreen','ayase.fullscreen-quiet','全屏安静','environment.fullscreen','quiet',true,'important'],
  ['media','ayase.media-response','媒体基础姿态','environment.media','music',false,'normal']
]){
  const m=manifest(id,name,['environment.read','base.claim','base.release'],{mode,suppress});m.defaultPriority=priority;
  write(`plugins/builtin/${short}.json`,wrap(m,{type:'object',required:['mode','suppress'],properties:{mode:{type:'string',minLength:1,maxLength:63,title:'基础姿态'},suppress:{type:'boolean',title:'抑制新主动互动'}}},
    [{id:'environment',type:event}], [{id:'on',on:'environment',when:'active',actions:[{type:'base.claim',modeFromConfig:'/mode',suppressFromConfig:'/suppress'}]},{id:'off',on:'environment',when:'inactive',actions:[{type:'base.release'}]}]));
}
const m=manifest('example.nightly-rest','夜间休息提醒',['clock.schedule','presentation.request'],{time:'23:00',timeZone:'system',missedPolicy:'skip',ttlSeconds:300,response:{mode:'fixed',variants:[{id:'rest-1',weight:1,text:'已经很晚了，记得休息。',cue:'reminder'}]}});
write('plugins/examples/nightly-rest/plugin.json',m);
write('plugins/examples/nightly-rest/schemas/config.schema.json',{type:'object',required:['time','timeZone','missedPolicy','ttlSeconds','response'],properties:{time:{...time,title:'每天提醒时间'},timeZone:{type:'string',title:'IANA 时区或 system'},missedPolicy:{enum:['skip','within-ttl'],title:'错过提醒时'},ttlSeconds:ttl,response}});
write('plugins/examples/nightly-rest/rules/main.json',{schemaVersion:'1.0',subscriptions:[{id:'daily-rest',type:'schedule.daily',timeFromConfig:'/time',timeZoneFromConfig:'/timeZone',missedPolicyFromConfig:'/missedPolicy'}],rules:[{id:'rest',on:'daily-rest',actions:[present]}]});
console.log('Generated three builtin rule sources and the standalone nightly-rest source.');
