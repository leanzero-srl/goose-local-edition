import { kvs } from '@forge/kvs';
import { jiraJson,route,postJson } from './jira';
import { toMicro,creepTenths } from './numbers';
export const CHANGES='scope-ledger';
export const changeKey=(changeId,sprintId)=>`${changeId}:${sprintId}`;
export const memberKey=(sprintId,issueId)=>`${sprintId}:${issueId}`;
export async function queryAll(entity,index,partition) {
  const out=[];let cursor;
  do{let q=kvs.entity(entity).query().index(index,{partition}).limit(100);if(cursor)q=q.cursor(cursor);const p=await q.getMany();out.push(...p.results.map(r=>r.value));cursor=p.nextCursor;}while(cursor);
  return out;
}
export const changesOfSprint=id=>queryAll(CHANGES,'by-sprint',[String(id)]);
export const changesOfIssue=id=>queryAll(CHANGES,'by-issue',[String(id)]);
export const membersOfSprint=async id=>Object.values(await kvs.get(`members:${id}`)??{});
export const compareIds=(a,b)=>/^\d+$/.test(a)&&/^\d+$/.test(b)?(BigInt(a)<BigInt(b)?-1:BigInt(a)>BigInt(b)?1:0):(a<b?-1:a>b?1:0);
export const byTime=(a,b)=>a.at-b.at||compareIds(a.changeId,b.changeId);
export function computeTotals(changes,members) {
 const byIssue=new Map();for(const c of [...changes].sort(byTime)){if(!byIssue.has(c.issueId))byIssue.set(c.issueId,[]);byIssue.get(c.issueId).push(c);}
 let committed=0,added=0,removed=0;
 const current=new Map(members.map(m=>[m.issueId,m]));
 for(const id of new Set([...current.keys(),...byIssue.keys()])){
  const m=current.get(id),history=byIssue.get(id)??[],inNow=m?.inSprint===true,atStart=history.length?history[0].kind==='removed':inNow,ever=atStart||history.some(c=>c.kind==='added');
  const points=toMicro(m?.deleted?0:m?.estimate);if(atStart)committed+=points;if(inNow&&!atStart)added+=points;if(ever&&!inNow)removed+=points;
 }
 return {committed,added,removed,creepTenths:creepTenths(added,committed)};
}
export async function sprintTotals(id) {const [changes,members]=await Promise.all([changesOfSprint(id),membersOfSprint(id)]);return {changes,members,totals:computeTotals(changes,members)};}
export async function visibleIssues(ids,policy) {
 const result=new Map();const unique=[...new Set(ids)];
 for(let i=0;i<unique.length;i+=100){const p=await jiraJson('user',route`/rest/api/3/issue/bulkfetch`,postJson({issueIdsOrKeys:unique.slice(i,i+100),fields:['key']}),policy);for(const x of p.issues??[])result.set(String(x.id),x.key);}
 return result;
}
export async function recordChange(row) {
 const key=changeKey(row.changeId,row.sprintId),entity=kvs.entity(CHANGES);
 if(await entity.get(key))return false;
 const old=await kvs.entity('scope-change').get(key);if(old)row={...row,...old};
 try{await entity.set(key,row,{keyPolicy:'FAIL_IF_EXISTS'});return true;}catch(e){if(await entity.get(key))return false;throw e;}
}
