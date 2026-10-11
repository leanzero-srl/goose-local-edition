import { kvs } from '@forge/kvs';
import { jiraJson,route,postJson,remaining,RateLimited,sleep } from './jira';
import { changesOfSprint } from './ledger';
import { loadConfig,resolveSprint } from './config';
import { REFRESH_WORK } from './pending';
import { announce } from './realtime';
export async function fragment(sprintId,operator){
 const rows=await changesOfSprint(sprintId),ids=[...new Set(rows.filter(r=>!r.deleted).map(r=>r.issueId))].sort((a,b)=>Number(a)-Number(b));
 if(ids.some(id=>!/^\d+$/.test(id)))throw new Error('Stored issue id is not numeric.');
 const groups=[];for(let i=0;i<ids.length;i+=1000)groups.push(`issue ${operator} (${ids.slice(i,i+1000).join(',')})`);
 return groups.length?`(${groups.join(operator==='not in'?' AND ':' OR ')})`:`issue ${operator} (0)`;
}
export async function scopeChanged(payload){
 try{const c=payload?.clause,id=c?.arguments?.[0];if(c?.arguments?.length!==1||typeof id!=='string'||!/^\d+$/.test(id))return {error:'scopeChanged requires one numeric sprintId.'};const op=String(c.operator).toLowerCase();if(!['in','not in'].includes(op))return {error:'scopeChanged supports IN and NOT IN.'};const cfg=await loadConfig()??{sprints:{}};const s=cfg.sprints[id]??await resolveSprint(cfg,id,{});if(!s){const res=await jiraJson('app',route`/rest/agile/1.0/sprint/${id}`);if(res.state!=='future')return {error:'Unknown sprint.'};}
 return {jql:await fragment(id,op)};}catch(e){return {error:`Could not expand scopeChanged: ${e.message}`};}
}
export async function refreshPrecomputations(policy){
 let work=await kvs.get(REFRESH_WORK);if(!work?.sprintIds.length)return;
 if(remaining()<8000)throw new RateLimited(1);
 // Ledger index queries can omit writes younger than five seconds. Wait after this invocation
 // acquired the serialized consumer slot, including when the previous invocation was killed.
 await sleep(5000);
 const wanted=new Set(work.sprintIds);
 for(;;){
  if(remaining()<3000)throw new RateLimited(1);
  if(!work.page){
   const p=await jiraJson('app',route`/rest/api/3/jql/function/computation?startAt=${work.startAt}&maxResults=100`,undefined,policy);
   if(!Array.isArray(p.values))throw new Error('JQL precomputation page has no values array.');
   work={...work,page:{values:p.values,isLast:Boolean(p.isLast)||p.values.length===0}};
   await kvs.set(REFRESH_WORK,work);
  }
  const values=[];
  for(const v of work.page.values)if(v.functionName==='scopeChanged'&&wanted.has(String(v.arguments?.[0])))values.push({id:v.id,value:await fragment(String(v.arguments[0]),String(v.operator).toLowerCase())});
  if(values.length)await jiraJson('app',route`/rest/api/3/jql/function/computation`,postJson({values}),policy);
  if(work.page.isLast){await announce(work.sprintIds);await kvs.delete(REFRESH_WORK);return;}
  work={sprintIds:work.sprintIds,startAt:work.startAt+work.page.values.length};
  await kvs.set(REFRESH_WORK,work);
 }
}
