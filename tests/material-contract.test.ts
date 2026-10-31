import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
const read=(path:string)=>JSON.parse(readFileSync(new URL('../'+path,import.meta.url),'utf8'));
test('每个内置固定/后备表单与共享多素材契约一致，示例也可编辑数组',()=>{
  const request=read('contracts/request.schema.json');
  for(const id of ['work-reminder','random-interaction','pointer-interaction','scheduled-reminder','observation','weather-care','calendar-care','special-date','routine-care']){
    assert.deepEqual(read(`plugins/builtin/${id}.json`).schema.properties.response,request);
  }
  assert.deepEqual(read('plugins/builtin/startup.json').schema.properties.windows.items.properties.response,request);
  assert.deepEqual(read('plugins/examples/nightly-rest/schemas/config.schema.json').properties.response,request);
});
