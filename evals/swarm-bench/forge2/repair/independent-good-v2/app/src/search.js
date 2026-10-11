import { jiraJson,route,postJson } from './jira';
import { changesOfSprint } from './ledger';
import { loadConfig,resolveSprint } from './config';
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
export async function refreshPrecomputations(ids,policy){
 const wanted=new Set(ids);let startAt=0;for(;;){const p=await jiraJson('app',route`/rest/api/3/jql/function/computation?startAt=${startAt}&maxResults=100`,undefined,policy);const values=[];for(const v of p.values??[])if(v.functionName==='scopeChanged'&&wanted.has(String(v.arguments?.[0])))values.push({id:v.id,value:await fragment(String(v.arguments[0]),String(v.operator).toLowerCase())});if(values.length)await jiraJson('app',route`/rest/api/3/jql/function/computation`,postJson({values}),policy);if(p.isLast||!p.values?.length)break;startAt+=p.values.length;}
}
