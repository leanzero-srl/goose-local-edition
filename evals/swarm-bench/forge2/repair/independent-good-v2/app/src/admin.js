import { randomBytes } from 'node:crypto';
import { kvs } from '@forge/kvs';
import { jiraJson,route } from './jira';
import { loadConfig } from './config';
import { queryAll } from './ledger';
export const defaults={share:70,aiEnabled:true,tokenBudget:200000,commentGroup:''};
export const settings=async()=>({...defaults,...await kvs.get('settings')});
export async function authorize(){const p=await jiraJson('user',route`/rest/api/3/mypermissions?permissions=ADMINISTER`);if(!p.permissions?.ADMINISTER?.havePermission)throw new Error('Jira ADMINISTER permission is required.');}
export async function adminView(){
 const stored=await settings();let migration=await kvs.get('migration');
 if(!migration){let total=0;const cfg=await loadConfig();for(const s of Object.values(cfg?.sprints??{}))total+=(await queryAll('scope-change','by-sprint',[s.id])).length;migration={index:0,total};}
 return {settings:stored,migration:`Migrated ${migration.index} of ${migration.total} v1 rows${migration.complete?' · complete':''}`,secret:`CI secret: ${await kvs.get('ci-tail')?'••••'+await kvs.get('ci-tail'):'not configured'}`,audit:await kvs.get('admin-audit')??[]};
}
async function audit(who,what){const rows=await kvs.get('admin-audit')??[];await kvs.set('admin-audit',[{when:new Date().toISOString(),who,what},...rows].slice(0,20));}
export async function saveSettings(payload,context){
 const numeric=value=>typeof value==='number'||(typeof value==='string'&&/^[0-9]+$/.test(value))?Number(value):NaN;
 const values={share:numeric(payload.share),aiEnabled:payload.aiEnabled,tokenBudget:numeric(payload.tokenBudget),commentGroup:String(payload.commentGroup??'').trim()};
 if(!Number.isInteger(values.share)||values.share<10||values.share>90||!Number.isInteger(values.tokenBudget)||values.tokenBudget<0||typeof values.aiEnabled!=='boolean')return {error:'Background share must be a whole number from 10 to 90; token budget must be a non-negative whole number.'};
 const old=await settings(),labels={share:'Background share (%)',aiEnabled:'AI explanations enabled',tokenBudget:'Daily AI token budget',commentGroup:'Comment group'},changed=Object.keys(labels).filter(k=>values[k]!==old[k]);
 if(changed.length){await kvs.set('settings',values);await audit(context.accountId,changed.map(k=>labels[k]).join(', '));}
 return adminView();
}
export async function rotateSecret(context){const secret=randomBytes(32).toString('hex');await kvs.setSecret('ci-secret',secret);await kvs.set('ci-tail',secret.slice(-4));await audit(context.accountId,'Rotate CI secret');return {...await adminView(),secret:`CI secret: ${secret}`};}
