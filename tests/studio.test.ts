import test from 'node:test';
import assert from 'node:assert/strict';
import {bubbleGeometry,defaultBubblePreferences,readingTime} from '../packages/presentation/bubble.ts';
import {displaySize} from '../packages/presentation/model.ts';
import {resolveSchema,schemaDefaults,schemaBranch} from '../packages/editor/schema-form.ts';
import {rgba} from '../packages/presentation/appearance.ts';

test('自定义气泡宽高可超过模型，但仍在可见视口；像素偏移保留方向',()=>{
 const model={x:220,y:420,width:160,height:200},view={width:700,height:760};
 const a=bubbleGeometry(model,view,1,{maxWidth:560,maxHeight:600,offsetX:60,offsetY:20});
 assert.equal(a.width,560);assert.ok(a.maxHeight>200);assert.ok(a.maxHeight<=view.height-a.bottom-12);assert.equal(a.bottom,view.height-(420-12+20));
 for(const x of [-1600,1600])for(const y of [-1600,1600]){const b=bubbleGeometry(model,view,1,{maxWidth:1600,maxHeight:1600,offsetX:x,offsetY:y});assert.ok(b.width<=view.width-24);assert.ok(b.maxHeight>=0&&b.maxHeight<view.height);assert.ok(b.center-b.width/2>=12);assert.ok(b.center+b.width/2<=view.width-12);}
});
test('静态图预览与实机保留横纵比，不拉伸新角色',()=>{assert.deepEqual(displaySize({width:100,height:200},1),{width:140,height:280});assert.deepEqual(displaySize({width:360,height:280},1),{width:360,height:280});});
test('共享表单支持动态字典、引用、空值与有类型枚举默认值',()=>{const root={$defs:{item:{type:'object',required:['id'],properties:{id:{type:'string'},enabled:{type:'boolean',default:true}}}}};const item=resolveSchema({$ref:'#/$defs/item'},root);assert.deepEqual(schemaDefaults(item),{id:'',enabled:true});assert.equal(schemaDefaults({anyOf:[{type:'string'},{type:'null'}]}),null);assert.equal(schemaBranch([{type:'null'},{type:'array'}],['a']),1);});
test('短素材依然遵循全局阅读下限，透明色仅生成背景颜色',()=>{assert.equal(readingTime(2,{...defaultBubblePreferences,readMs:6000},1000),6000);assert.equal(rgba('#123456',25),'rgba(18,52,86,0.25)');});
