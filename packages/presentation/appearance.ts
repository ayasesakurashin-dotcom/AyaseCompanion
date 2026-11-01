export interface Appearance {
 foreground:string;background:string;borderColor:string;fillOpacity:number;borderRadius:number;
 paddingTop:number;paddingRight:number;paddingBottom:number;paddingLeft:number;tailSize:number;tailX:number;
 fontFamily:string;offsetX:number;offsetY:number;maxWidth:number|null;maxHeight:number|null;inputOpacity:number;
 backgroundAsset:string|null;sliceTop:number;sliceRight:number;sliceBottom:number;sliceLeft:number;
}
export const defaultAppearance:Appearance={foreground:'#493c4d',background:'#fffafe',borderColor:'#dfc8da',fillOpacity:98,borderRadius:16,paddingTop:12,paddingRight:16,paddingBottom:12,paddingLeft:16,tailSize:12,tailX:50,fontFamily:'Segoe UI',offsetX:0,offsetY:0,maxWidth:null,maxHeight:null,inputOpacity:100,backgroundAsset:null,sliceTop:12,sliceRight:12,sliceBottom:12,sliceLeft:12};
export function rgba(hex:string,opacity:number){const n=parseInt(hex.slice(1),16);return `rgba(${n>>16&255},${n>>8&255},${n&255},${opacity/100})`;}
export function applyAppearance(element:HTMLElement,value:Partial<Appearance>={},backgroundUrl?:string){
 const a={...defaultAppearance,...value};element.style.setProperty('--bubble-bg',rgba(a.background,a.fillOpacity));element.style.setProperty('--bubble-border',a.borderColor);element.style.setProperty('--tail-size',`${a.tailSize}px`);element.style.setProperty('--tail-x',`${a.tailX}%`);
 element.style.color=a.foreground;element.style.borderColor=a.borderColor;element.style.borderRadius=`${a.borderRadius}px`;element.style.padding=`${a.paddingTop}px ${a.paddingRight}px ${a.paddingBottom}px ${a.paddingLeft}px`;element.style.fontFamily=`"${a.fontFamily}","Microsoft YaHei",sans-serif`;
 element.style.borderImageSource=backgroundUrl?`url("${backgroundUrl}")`:'none';element.style.borderImageSlice=backgroundUrl?`${a.sliceTop} ${a.sliceRight} ${a.sliceBottom} ${a.sliceLeft} fill`:'';element.style.borderImageWidth=backgroundUrl?`${a.sliceTop}px ${a.sliceRight}px ${a.sliceBottom}px ${a.sliceLeft}px`:'';
 return a;
}
