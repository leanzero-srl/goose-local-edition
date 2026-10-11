import { kvs } from '@forge/kvs';
import { Queue } from '@forge/events';
import { jiraJson,route,postJson,remaining,RateLimited,sleep } from './jira';
import { loadConfig,saveConfig,discoverConfig,resolveSprint,refreshBoard } from './config';
import { queryAll,changesOfSprint,recordChange,changeKey } from './ledger';
import { announce } from './realtime';
import { refreshPrecomputations } from './search';
export const background={background:true};
export const enqueue=(body)=>new Queue({key:'scope-ledger'}).push({body,concurrency:{key:'scope-background',limit:1}});
export const estimateFieldIds=cfg=>[...new Set([...(cfg.estimateFields??[]),...Object.values(cfg.sprints).flatMap(s=>s.fieldHistory.map(h=>h.fieldId))].filter(Boolean))];
export const issueFields=cfg=>[cfg.sprintFieldId,...estimateFieldIds(cfg),cfg.statusFieldId].filter(Boolean);
export const sprintsOfField=v=>Array.isArray(v)?v.map(s=>String(typeof s==='object'?s.id:s)):[];
const ids=v=>v==null?[]:String(v).split(',').map(x=>x.trim()).filter(Boolean);
export const instantOf=v=>typeof v==='number'?v:Date.parse(v);
export function sprintMoves(h,field){const out=[];for(const i of h.items??[]){if(i.fieldId?i.fieldId!==field:i.field!=='Sprint')continue;const from=new Set(ids(i.from)),to=new Set(ids(i.to));for(const id of to)if(!from.has(id))out.push({sprintId:id,kind:'added'});for(const id of from)if(!to.has(id))out.push({sprintId:id,kind:'removed'});}return out;}
const estimate=(issue,field)=>Number.isFinite(Number(issue.fields?.[field]))?Number(issue.fields[field]):0;
function rowFor(cfg,s,h,issue,kind,source,envs){const at=instantOf(h.created),field=[...s.fieldHistory].reverse().find(x=>x.at<=at)?.fieldId??s.estimateFieldId;return {sprintId:s.id,changeId:String(h.id),at,created:new Date(at).toISOString(),kind,issueId:String(issue.id),issueKey:issue.key,authorId:h.author?.accountId??'',authorName:h.author?.displayName??'',source,estimate:estimate(issue,field),estimateField:field??'',boardId:s.boardId,deleted:false,deployedEnvs:envs};}
async function allHistories(issue,cfg,policy){
 const embedded=issue.changelog?.histories??[];if(issue.changelog&&embedded.length>=(issue.changelog.total??embedded.length))return embedded;
 const out=[];let token;do{const body={issueIdsOrKeys:[String(issue.id)],fieldIds:[cfg.sprintFieldId,...estimateFieldIds(cfg)].slice(0,10),maxResults:1000};if(token)body.nextPageToken=token;const p=await jiraJson('app',route`/rest/api/3/changelog/bulkfetch`,postJson(body),policy);for(const log of p.issueChangeLogs??[])out.push(...log.changeHistories);token=p.nextPageToken;}while(token);return out;
}
async function writeChanged(key,value){const old=await kvs.get(key);if(JSON.stringify(old)===JSON.stringify(value))return false;await kvs.set(key,value);return true;}
async function fieldValue(cfg,issue,histories){
 const current=new Set(sprintsOfField(issue.fields?.[cfg.sprintFieldId]));
 for(const s of Object.values(cfg.sprints).filter(s=>s.state==='active')){
 const moves=histories.flatMap(h=>sprintMoves(h,cfg.sprintFieldId).filter(m=>m.sprintId===s.id).map(m=>({...m,at:instantOf(h.created),changeId:String(h.id)}))).filter(m=>m.at>s.startMs).sort((a,b)=>a.at-b.at||Number(a.changeId)-Number(b.changeId));
 if(current.has(s.id)) {if(!moves.length||moves[0].kind==='removed')return 'committed';const last=[...moves].reverse().find(m=>m.kind==='added');const row=last&&await kvs.entity('scope-ledger').get(changeKey(last.changeId,s.id));return `added +${row?.estimate??0}`;}
 }
 for(const s of Object.values(cfg.sprints).filter(s=>s.state==='active'))if(histories.some(h=>instantOf(h.created)>s.startMs&&sprintMoves(h,cfg.sprintFieldId).some(m=>m.sprintId===s.id)))return 'removed';
 return '';
}
export async function processIssue(cfg,issue,histories,source){
 const id=String(issue.id),envs=(await kvs.get(`deploy:${id}`)??[]).join(','),changed=new Set();
 for(const h of histories){const at=instantOf(h.created);if(!Number.isFinite(at))throw new Error(`Invalid changelog time for ${h.id}`);for(const move of sprintMoves(h,cfg.sprintFieldId)){
 const s=cfg.sprints[move.sprintId];if(!s||!(at>s.startMs)||(s.closeMs&&at>s.closeMs))continue;
 if(await recordChange(rowFor(cfg,s,h,issue,move.kind,source,envs)))changed.add(s.id);
 }}
 const current=new Set(sprintsOfField(issue.fields?.[cfg.sprintFieldId]));
 for(const s of Object.values(cfg.sprints)){
 if(s.state!=='active')continue;
 const relevant=current.has(s.id)||histories.some(h=>instantOf(h.created)>s.startMs&&sprintMoves(h,cfg.sprintFieldId).some(m=>m.sprintId===s.id));
 const key=`members:${s.id}`,members=await kvs.get(key)??{};
 if(!relevant&&!members[id])continue;
 const next={sprintId:s.id,issueId:id,issueKey:issue.key,inSprint:current.has(s.id),estimate:estimate(issue,s.estimateFieldId),deleted:false};
 if(JSON.stringify(members[id])!==JSON.stringify(next)){members[id]=next;await kvs.set(key,members);changed.add(s.id);}
 }
 const desired=await fieldValue(cfg,issue,histories);
 if(cfg.statusFieldId&&(issue.fields?.[cfg.statusFieldId]??'')!==desired){
 const init=postJson({updates:[{issueIds:[Number(id)],value:desired||null}]});init.method='PUT';
 await jiraJson('app',route`/rest/api/3/app/field/${cfg.statusFieldId}/value`,init,background);issue.fields[cfg.statusFieldId]=desired;
 }
 await writeChanged(`issue:${id}`,{...issue,changelog:{histories,total:histories.length}});
 return [...changed];
}
export async function removeIssue(cfg,id){
 const touched=[];
 for(const s of Object.values(cfg.sprints)){
 const members=await kvs.get(`members:${s.id}`)??{};
 if(members[id]&&s.state==='active'){members[id]={...members[id],inSprint:false,estimate:0,deleted:true};await kvs.set(`members:${s.id}`,members);touched.push(s.id);}
 for(const row of await changesOfSprint(s.id))if(row.issueId===id&&!row.deleted){await kvs.entity('scope-ledger').set(changeKey(row.changeId,row.sprintId),{...row,deleted:true});if(!touched.includes(s.id))touched.push(s.id);}
 }
 await kvs.set(`deleted:${id}`,true);return touched;
}
export async function applyIssueEvent(body){
 let cfg=await loadConfig();if(!cfg)cfg=await discoverConfig(background);
 if(body.deleted){const touched=await removeIssue(cfg,body.issueId);await finish(touched);return {deleted:true};}
 let issue;try{issue=await jiraJson('app',route`/rest/api/3/issue/${body.issueId}?fields=${issueFields(cfg).join(',')}&expand=changelog`,undefined,background);}catch(e){if(e.status===404){const touched=await removeIssue(cfg,body.issueId);await finish(touched);return {deleted:true};}throw e;}
 const histories=await allHistories(issue,cfg,background);
 const sprintIds=new Set([...sprintsOfField(issue.fields[cfg.sprintFieldId]),...histories.flatMap(h=>sprintMoves(h,cfg.sprintFieldId).map(m=>m.sprintId))]);
 for(const id of sprintIds)if(!cfg.sprints[id])await resolveSprint(cfg,id,background);
 for(const id of sprintIds){const known=cfg.sprints[id];if(known?.state!=='active')continue;const actual=await jiraJson('app',route`/rest/agile/1.0/sprint/${id}`,undefined,background);if(actual.state!=='active'){known.state=actual.state;known.closeMs=Date.parse(actual.completeDate??actual.endDate);}}
 for(const boardId of new Set([...sprintIds].map(id=>cfg.sprints[id]).filter(s=>s?.state==='active').map(s=>s.boardId)))await refreshBoard(cfg,boardId,background,Math.max(...histories.map(h=>instantOf(h.created))));
 await saveConfig(cfg);const touched=await processIssue(cfg,issue,histories,'event');await finish(touched);return {sprintIds:touched};
}
async function finish(ids){if(!ids.length)return;await sleep(5000);await refreshPrecomputations(ids,background);await announce(ids);}
const jqlDate=ms=>new Date(ms).toISOString().slice(0,16).replace('T',' ');
export async function reconcileBatch(){
 let work=await kvs.get('reconcile-work'),cfg;
 if(!work){cfg=await discoverConfig(background);const last=await kvs.get('last-reconcile');const start=Date.now();const sprints=Object.values(cfg.sprints);const earliest=Math.min(...sprints.filter(s=>s.state==='active').map(s=>s.startMs));const since=last??earliest-86400000;
 const clause=sprints.map(s=>s.id).join(',');work={jql:`${clause?`sprint in (${clause}) OR `:''}updated >= "${jqlDate(since-60000)}"`,started:start,token:null,page:null,index:0,changed:[]};await kvs.set('reconcile-work',work);}
 else cfg=await loadConfig();
 if(!work.page){const body={jql:work.jql,fields:issueFields(cfg),expand:'changelog',maxResults:25};if(work.token)body.nextPageToken=work.token;const p=await jiraJson('app',route`/rest/api/3/search/jql`,postJson(body),background);work.page=p.issues;work.nextToken=p.isLast?null:p.nextPageToken??null;work.index=0;await kvs.set('reconcile-work',work);}
 for(;work.index<work.page.length;work.index++){
 if(remaining()<20000){await kvs.set('reconcile-work',work);await enqueue({kind:'reconcile'});return {continued:true};}
 const issue=work.page[work.index],history=await allHistories(issue,cfg,background);
 const touched=await processIssue(cfg,issue,history,'reconcile');work.changed=[...new Set([...work.changed,...touched])];
 await kvs.set('reconcile-work',{...work,index:work.index+1});
 }
 if(work.nextToken){work.token=work.nextToken;work.page=null;work.index=0;await kvs.set('reconcile-work',work);await enqueue({kind:'reconcile'});return {continued:true};}
 await kvs.set('last-reconcile',work.started);await kvs.delete('reconcile-work');
 await migrate(cfg);await applyPendingDeployments(cfg);await finish(Object.keys(cfg.sprints));return {complete:true};
}
async function migrate(cfg){
 let progress=await kvs.get('migration');
 if(!progress){const rows=[];for(const s of Object.values(cfg.sprints))rows.push(...await queryAll('scope-change','by-sprint',[s.id]));progress={rows,index:0,total:rows.length};await kvs.set('migration',progress);}
 for(;progress.index<progress.rows.length;progress.index++){
 if(remaining()<10000){await kvs.set('migration',progress);await enqueue({kind:'migration'});return;}
 const old=progress.rows[progress.index],s=cfg.sprints[old.sprintId];if(!s)throw new Error(`Migration cannot resolve sprint ${old.sprintId}`);
 const cached=await kvs.get(`issue:${old.issueId}`),deleted=await kvs.get(`deleted:${old.issueId}`)===true;
 const field=s.fieldHistory[0].fieldId,envs=(await kvs.get(`deploy:${old.issueId}`)??[]).join(',');
 await recordChange({...old,estimate:cached?estimate(cached,field):0,estimateField:field??'',boardId:s.boardId,deleted,deployedEnvs:envs});
 await kvs.set('migration',{...progress,index:progress.index+1});
 }
 await kvs.set('migration',{...progress,complete:true});
}
export async function migrateOnly(){const cfg=await loadConfig();await migrate(cfg);await finish(Object.keys(cfg.sprints));}
export async function applyPendingDeployments(cfg,eventId){
 const direct=eventId?await kvs.entity('ci-deployment').get(eventId):null;
 const receipts=direct?[direct]:await queryAll('ci-deployment','by-state',['pending']);
 for(const receipt of receipts){
 if(receipt.state!=='pending')continue;
 for(const issueKey of receipt.issueKeys.split(',')){
  const matches=await jiraJson('app',route`/rest/api/3/issue/${issueKey}?fields=key`,undefined,background).catch(e=>{if(e.status===404)return null;throw e;});if(!matches)continue;
  const id=String(matches.id),envs=[...new Set([...(await kvs.get(`deploy:${id}`)??[]),receipt.environment])];await kvs.set(`deploy:${id}`,envs);
  for(const s of Object.values(cfg.sprints))for(const row of await changesOfSprint(s.id))if(row.issueId===id&&!row.deployedEnvs.split(',').includes(receipt.environment))await kvs.entity('scope-ledger').set(changeKey(row.changeId,row.sprintId),{...row,deployedEnvs:envs.join(',')});
 }
 await kvs.entity('ci-deployment').set(receipt.eventId,{...receipt,state:'done'});
 }
}
