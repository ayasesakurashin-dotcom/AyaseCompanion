type Json=Record<string,any>;
export interface FormOptions{model?:{clips:Record<string,{asset:string}>}|null;labels?:Record<string,string>}
export function schemaDefaults(schema:Json):any{
 if(schema.default!==undefined)return structuredClone(schema.default);if(schema.const!==undefined)return schema.const;
 if(schema.properties?.mode?.enum?.includes('generated-with-fallback')&&schema.properties?.variants)return {mode:'fixed',variants:[]};
 if(schema.enum)return schema.enum[0];if(schema.anyOf?.some((s:Json)=>s.type==='null'))return null;
 if(schema.type==='object')return Object.fromEntries(Object.entries(schema.properties??{}).filter(([k,v])=>schema.required?.includes(k)||(v as Json).default!==undefined).map(([k,v])=>[k,schemaDefaults(v as Json)]));
 if(schema.type==='array')return [];if(schema.type==='boolean')return false;if(['integer','number'].includes(schema.type))return schema.minimum??0;return '';
}
export function resolveSchema(schema:Json,root:Json):Json{
 if(!schema.$ref)return schema;const key=String(schema.$ref).replace(/^#\/\$defs\//,'');return {...root.$defs?.[key],...Object.fromEntries(Object.entries(schema).filter(([k])=>k!=='$ref'))};
}
export function schemaBranch(branches:Json[],value:any):number{
 const match=(s:Json)=>s.const!==undefined?s.const===value:s.type==='null'?value===null:s.type==='array'?Array.isArray(value):s.type==='object'?!!value&&typeof value==='object'&&!Array.isArray(value)&&Object.entries(s.properties??{}).every(([k,v])=>(v as Json).const===undefined||value[k]===(v as Json).const):s.type==='integer'?Number.isInteger(value):s.type==='number'?typeof value==='number':s.type==='boolean'?typeof value==='boolean':s.type==='string'?typeof value==='string':true;
 return Math.max(0,branches.findIndex(match));
}
export function renderSchemaForm(container:HTMLElement,root:Json,initial:any,onChange:(v:any)=>void,options:FormOptions={}){
 const defs:Json={};const gather=(s:any)=>{if(!s||typeof s!=='object')return;if(s.$defs)Object.assign(defs,s.$defs);Object.values(s).forEach(gather);};gather(root);root={...root,$defs:defs};
 const prefix='form-'+crypto.randomUUID();let value=structuredClone(initial);let seq=0;
 function draw(){container.replaceChildren();field(root,value,'参数',v=>{value=v;onChange(v);},container,0);}
 function field(raw:Json,current:any,key:string,change:(v:any)=>void,parent:HTMLElement,depth:number){
  let schema=resolveSchema(raw,root);const wrap=document.createElement('div');wrap.className='field';parent.append(wrap);
  const label=document.createElement('label');label.id=`${prefix}-${++seq}`;label.textContent=schema.title??options.labels?.[key]??key;wrap.append(label);
  const hint=()=>{if(schema.description){const small=document.createElement('small');small.textContent=schema.description;wrap.append(small);}};
  if(depth>24){wrap.append(document.createTextNode('配置嵌套过深'));return;}
  if(schema.anyOf){const nullable=schema.anyOf.some((s:Json)=>s.type==='null');const branches=schema.anyOf.filter((s:Json)=>s.type!=='null');
   if(nullable){const line=document.createElement('label');line.className='switch';const enabled=document.createElement('input');enabled.type='checkbox';enabled.checked=current!==null&&current!==undefined;line.append(enabled,document.createTextNode('指定此项（关闭为自动或不使用）'));wrap.append(line);enabled.onchange=()=>{change(enabled.checked?schemaDefaults(branches[0]):null);draw();};if(!enabled.checked){hint();return;}}
   const selected=branches[schemaBranch(branches,current)]??branches[0];schema={...schema,...selected};
   if(key==='text'&&!Array.isArray(current)&&branches.some((s:Json)=>s.type==='array')){const multi=document.createElement('button');multi.type='button';multi.textContent='改为多段文字';multi.onclick=()=>{change(current?[current]:[]);draw();};wrap.append(multi);}
  }
  if(schema.oneOf){const i=schemaBranch(schema.oneOf,current),picker=document.createElement('select');schema.oneOf.forEach((s:Json,n:number)=>picker.add(new Option(s.title??`类型 ${n+1}`,String(n))));picker.value=String(i);picker.onchange=()=>{change(schemaDefaults(schema.oneOf[Number(picker.value)]));draw();};wrap.append(picker);schema={...schema,...schema.oneOf[i]};}
  let read=()=>current;
  const bind=(element:HTMLElement)=>{element.setAttribute('aria-labelledby',label.id);wrap.append(element);};
  if(schema.const!==undefined){const input=document.createElement('input');input.value=String(schema.const);input.readOnly=true;bind(input);}
  else if(schema.enum){const input=document.createElement('select');for(const v of schema.enum)input.add(new Option(v===null?'通用（所有状态）':v==='state-specific'?'state-specific · 按基础状态选择':String(v),String(v)));input.value=String(current);input.onchange=()=>change(schema.enum.find((v:any)=>String(v)===input.value));bind(input);}
  else if(schema.type==='object'){
   const obj=current&&typeof current==='object'&&!Array.isArray(current)?structuredClone(current):{};const group=document.createElement('fieldset');wrap.append(group);const reply=schema.properties?.mode?.enum?.includes('generated-with-fallback')&&schema.properties?.variants;
   for(const[k,s]of Object.entries(schema.properties??{})){
    if(reply&&((obj.mode==='state-specific'&&!['mode','stateResponses'].includes(k))||(obj.mode!=='state-specific'&&(k==='stateResponses'||(k==='variants'&&obj.mode!=='fixed')||(k==='fallback'&&obj.mode!=='generated-with-fallback')||(['intent','voice','cue','animationAsset'].includes(k)&&obj.mode==='fixed')))))continue;
    field(s as Json,obj[k]===undefined?schemaDefaults(s as Json):obj[k],k,v=>{obj[k]=v;if(reply&&k==='mode'){for(const name of ['variants','fallback'])if(Array.isArray(obj[name])&&!obj[name].length)delete obj[name];if(!obj.intent)delete obj.intent;if(v==='state-specific')obj.stateResponses??=[];else if(v==='fixed')obj.variants??=[];else{obj.intent??='';if(v==='generated-with-fallback')obj.fallback??=[];}}change(obj);if(reply&&k==='mode')draw();},group,depth+1);
   }
   if(schema.additionalProperties&&schema.additionalProperties!==false){
    const extra=typeof schema.additionalProperties==='object'?schema.additionalProperties:{};
    for(const name of Object.keys(obj).filter(k=>!schema.properties?.[k])){const row=document.createElement('div');row.className='map-item';group.append(row);field(extra,obj[name],name,v=>{obj[name]=v;change(obj);},row,depth+1);const remove=document.createElement('button');remove.type='button';remove.textContent=`删除 ${name}`;remove.onclick=()=>{delete obj[name];change(obj);draw();};row.append(remove);}
    const name=document.createElement('input');name.placeholder='新字段或编号';name.setAttribute('aria-label',`${key} 新字段编号`);const add=document.createElement('button');add.type='button';add.textContent='添加字段';add.onclick=()=>{if(!name.value.trim()||['__proto__','constructor','prototype'].includes(name.value.trim())||Object.hasOwn(obj,name.value.trim()))return;obj[name.value.trim()]=schemaDefaults(extra);change(obj);draw();};group.append(name,add);
   }
  }else if(schema.type==='array'){
   const arr=Array.isArray(current)?structuredClone(current):[];read=()=>arr;
   arr.forEach((entry:any,index:number)=>{const row=document.createElement('div');row.className='array-item';wrap.append(row);field(schema.items??{},entry,`条目 ${index+1}`,v=>{arr[index]=v;change(arr);},row,depth+1);const remove=document.createElement('button');remove.type='button';remove.textContent='删除此项';remove.onclick=()=>{arr.splice(index,1);change(arr);draw();};row.append(remove);});
   const add=document.createElement('button');add.type='button';add.textContent='添加条目';add.disabled=arr.length>=(schema.maxItems??1000);add.onclick=()=>{const item=schemaDefaults(resolveSchema(schema.items??{},root));if(item&&typeof item==='object'&&!Array.isArray(item)&&schema.items?.properties?.id)item.id=crypto.randomUUID();arr.push(item);change(arr);draw();};wrap.append(add);
  }else if(schema.type==='boolean'){const input=document.createElement('input');input.type='checkbox';input.checked=!!current;input.onchange=()=>change(input.checked);bind(input);}
  else if(schema.type==='number'||schema.type==='integer'){const input=document.createElement('input');input.type='number';input.value=String(current??schema.minimum??0);input.step=schema.type==='integer'?'1':'any';if(schema.minimum!==undefined)input.min=String(schema.minimum);if(schema.maximum!==undefined)input.max=String(schema.maximum);input.oninput=()=>change(Number(input.value));bind(input);}
  else if(schema.type==='string'||typeof current==='string'){
   const input=document.createElement(schema.maxLength>=1000||['text','intent','persona','cue','animationAsset','audioAsset','textAsset'].includes(key)?'textarea':'input') as HTMLInputElement|HTMLTextAreaElement;input.value=String(current??'');if(schema.maxLength)input.maxLength=schema.maxLength;input.oninput=()=>change(input.value);read=()=>input.value;bind(input);
  }else{const input=document.createElement('textarea');input.rows=4;input.value=JSON.stringify(current??null,null,2);input.onchange=()=>{try{change(JSON.parse(input.value));input.setCustomValidity('');}catch{input.setCustomValidity('请输入有效JSON');}};bind(input);}
  if(key==='animationAsset'&&options.model){const pick=document.createElement('select');pick.setAttribute('aria-label','添加当前模型动画');pick.add(new Option('选择动画片段添加到末尾…',''));Object.entries(options.model.clips).forEach(([k,v])=>pick.add(new Option(`${k} · ${v.asset}`,k)));pick.onchange=()=>{if(!pick.value)return;const v=read();change(Array.isArray(v)?[...v,pick.value]:[String(v??'').trim(),pick.value].filter(Boolean).join('\n'));draw();};wrap.append(pick);}
  hint();
 }
 draw();
}
