// Build-time data only. No code from an imported plugin is executed.
import {readFileSync,writeFileSync} from 'node:fs';
const read=p=>JSON.parse(readFileSync(p,'utf8'));
const save=(p,v)=>writeFileSync(p,JSON.stringify(v,null,2)+'\n');
const response=read('contracts/request.schema.json');
const str=(title,def='',max=160)=>({type:'string',title,default:def,maxLength:max});
const num=(title,def,min=1,max=86400)=>({type:'integer',title,default:def,minimum:min,maximum:max});
const bool=(title,def=false)=>({type:'boolean',title,default:def});
const choice=(title,values,def=values[0])=>({type:'string',title,enum:values,default:def});
const time=(title,def)=>({...str(title,def),pattern:'^([01][0-9]|2[0-3]):[0-5][0-9]$'});
const common={timeZone:str('IANA 时区或 system','system'),start:time('允许时段开始','08:00'),end:time('允许时段结束','23:00'),cooldownSeconds:num('此配置冷却（秒）',2700,1),dailyLimit:num('此配置每日上限',2,1,100),ttlSeconds:num('排队有效期（秒）',180,1,3600),response};
const specs=[
 ['scheduled-reminder','定时或周期提醒',{...common,start:time('允许时段开始','00:00'),end:time('允许时段结束','00:00'),time:time('固定提醒时间','23:00'),repeat:choice('重复方式',['daily','weekdays','interval']),days:{type:'array',title:'星期（1为周一，7为周日）',default:[1,2,3,4,5],minItems:1,maxItems:7,items:{type:'integer',minimum:1,maximum:7}},intervalSeconds:num('周期长度（秒）',3600,1),missedPolicy:choice('错过策略',['skip','within-ttl'])},'到用户配置的提醒时间了，按设定内容简短提醒。'],
 ['work-reminder','工作与休息提醒',{...common,basis:choice('计时口径',['app-runtime','device-active','manual']),thresholdSeconds:num('累计阈值（秒）',3600),restSeconds:num('连续休息重置（秒）',300),activeWithinSeconds:num('设备活跃判定（秒）',60,1,600)},'按当前计时口径提醒短暂休息，不声称知道用户一直在工作。'],
 ['random-interaction','随机互动',{...common,minSeconds:num('最短间隔（秒）',1800),maxSeconds:num('最长间隔（秒）',3600)},'自然地和用户简短聊一句。'],
 ['pointer-interaction','点击互动',{...common,click:choice('点击方式',['double','single']),area:str('命中区域 ID（空或模型无对应区域时使用整体）'),dailyLimit:num('每日上限',100,1,100),cooldownSeconds:num('点击冷却（秒）',5)},'回应用户对模型的点击。'],
 ['observation','定时观察',{...common,intervalSeconds:num('观察间隔（秒）',900,30),reply:bool('观察后主动评论',false)},'根据本次屏幕事实做简短评论，承认无法判断的内容。'],
 ['weather-care','天气关怀',{...common,lowC:{type:'number',title:'低温阈值（摄氏）',default:5,minimum:-60,maximum:60},highC:{type:'number',title:'高温阈值（摄氏）',default:32,minimum:-60,maximum:60},rain:bool('雨雪提醒',true)},'根据有地点和时间的天气预报适当关怀，明确是预报，不编造天气或用户位置。'],
 ['calendar-care','日历关怀',{...common,categories:{type:'array',title:'参与的日期类别',default:['traditional','public'],maxItems:4,items:{enum:['traditional','public','solar-term','workday']}}},'根据已导入日历里的日期作简短问候，不编造节假日或调休。'],
 ['special-date','特殊日期关怀',{...common},'根据用户明确保存的特殊日期作简短问候，不推测其他私人事实。'],
 ['routine-care','作息关怀',{...common,start:time('作息窗口开始','23:00'),end:time('作息窗口结束','07:00'),activeWithinSeconds:num('只在设备最近活跃时提醒（秒）',300,1,3600)},'当前处于用户设置的晚间窗口，可以轻声提醒休息，不断言用户失眠或熬夜。']
];
for(const [id,name,properties,intent] of specs){
 const defaults=Object.fromEntries(Object.entries(properties).map(([k,s])=>[k,s.default]));
 defaults.response={mode:'generated-with-fallback',intent,fallback:[{id:'default',text:id==='scheduled-reminder'?'到设定的提醒时间了。':id==='work-reminder'?'计时已经到设定的阈值了，要不要休息一下？':id==='pointer-interaction'?'我在这里。':'愿你今天也顺顺利利。'}]};
 save(`plugins/builtin/${id}.json`,{manifest:{format:'ayase.plugin',formatVersion:'1.0',pluginId:`ayase.${id}`,version:'0.3.0',displayName:name,apiVersion:'1.0',configVersion:'1.0',runtime:{type:'declarative.v1',entry:'rules/main.json'},configSchema:'schemas/config.schema.json',defaultPriority:'background',capabilities:['presentation.request',...({'scheduled-reminder':['clock.schedule'],observation:['screen.observe'],'weather-care':['weather.read'],'calendar-care':['calendar.read'],'special-date':['calendar.read']}[id]??[])],hostBehavior:id,defaults},schema:{type:'object',additionalProperties:false,required:Object.keys(properties),properties},rules:{schemaVersion:'1.0',subscriptions:[],rules:[]}});
}
