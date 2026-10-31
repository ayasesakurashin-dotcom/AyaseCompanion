import type {Model} from '../../../packages/presentation/model.ts';
import type {BubblePreferences} from '../../../packages/presentation/bubble.ts';
export type Json=Record<string,any>;
export interface Task{id:number;generation:number;instanceId:string;owner:string;priority:string;status:string;plan:{preparationStatus?:string;text?:string;audioPath?:string;audioDurationMs?:number;clips?:string[];repeat?:number;readMs?:number;serviceError?:string;stream?:boolean;streamDone?:boolean;audioSegments?:({path?:string;failed?:boolean;spokenText?:string}|null)[];audioDone?:boolean;audioCount?:number};channels:Record<string,string>;targetBase?:string;result?:string}
export interface BaseTransition{id:number;generation:number;instanceId:string;parentTaskId:number;fromBase:string;targetBase:string;clips:string[];status:string}
export interface Snapshot {
  petInteractive?:boolean;
  version:string;stage:string;instance:Json|null;instances:Json[];package:Json|null;manifest:Json|null;library:Json[];
  model:Model|null;assetPaths:Record<string,string>;plugins:Json[];configured:Json[];unknownPresets:Json[];
  scheduler:{active:Task|null;tasks:Task[];desiredBase:string;appliedBase:string;baseTransition?:BaseTransition|null;suppressed:boolean;muted:boolean;log:Json[];claims:Json[]};
  facts:Json;zoom:number;bubble:BubblePreferences;excluded:string[];dataDirectory:string;
  ai?:Json;
  p3?:Json;
}
