import { kvs } from '@forge/kvs';
import { jiraJson, route } from './jira';
export const loadConfig = () => kvs.get('config-v2');
export const isStarted = (s) => s.state !== 'future' && Boolean(s.startDate);
export const saveConfig = (cfg) => kvs.set('config-v2', cfg);
async function list(path, policy) {
  const all=[]; for(let offset=0;;) { const p=await jiraJson('app',path(offset),undefined,policy); all.push(...p.values); if(p.isLast || !p.values.length)return all; offset+=p.values.length; }
}
export const listScrumBoards = (policy) => list((n)=>route`/rest/agile/1.0/board?type=scrum&startAt=${n}&maxResults=50`,policy);
export const listActiveSprints = (id,policy) => list((n)=>route`/rest/agile/1.0/board/${id}/sprint?state=active&startAt=${n}&maxResults=50`,policy);
export async function boardEstimateField(id,policy) { const c=await jiraJson('app',route`/rest/agile/1.0/board/${id}/configuration`,undefined,policy); return c.estimation?.field?.fieldId ?? null; }
export async function refreshBoard(cfg,boardId,policy,effectiveAt=Date.now()) {
  const field=await boardEstimateField(boardId,policy);
  for(const s of Object.values(cfg.sprints))if(s.boardId===String(boardId)&&s.state==='active'&&s.estimateFieldId!==field) {s.fieldHistory.push({at:effectiveAt,fieldId:field});s.estimateFieldId=field;}
  return field;
}
export async function discoverConfig(policy) {
  const cfg=await loadConfig() ?? {sprints:{},boards:[]};
  const fields=await jiraJson('app',route`/rest/api/3/field`,undefined,policy);
  const sf=fields.find(f=>f.schema?.custom==='com.pyxis.greenhopper.jira:gh-sprint'); if(!sf)throw new Error('Sprint field was not found.');
  cfg.sprintFieldId=sf.id;cfg.statusFieldId=fields.find(f=>f.name==='Scope status')?.id;
  cfg.estimateFields=fields.filter(f=>f.schema?.type==='number').map(f=>f.id);
  cfg.boards=(await listScrumBoards(policy)).map(b=>({id:String(b.id),name:b.name}));
  const active=new Set();
  for(const b of cfg.boards) {
    const field=await refreshBoard(cfg,b.id,policy);
    for(const s of await listActiveSprints(b.id,policy)) {
      const id=String(s.id);active.add(id);
      cfg.sprints[id]={...(cfg.sprints[id]??{}),id,name:s.name,state:s.state,startDate:s.startDate,startMs:Date.parse(s.startDate),boardId:String(s.originBoardId??b.id),estimateFieldId:field,fieldHistory:cfg.sprints[id]?.fieldHistory??[{at:0,fieldId:field}]};
    }
  }
  for(const s of Object.values(cfg.sprints))if(s.state==='active'&&!active.has(s.id)) {
    const current=await jiraJson('app',route`/rest/agile/1.0/sprint/${s.id}`,undefined,policy);
    s.state=current.state;s.closeMs=Date.parse(current.completeDate??current.endDate);s.completeDate=current.completeDate;
  }
  await saveConfig(cfg);return cfg;
}
export async function resolveSprint(cfg,id,policy) {
  if(cfg.sprints[id])return cfg.sprints[id];
  let s;try{s=await jiraJson('app',route`/rest/agile/1.0/sprint/${id}`,undefined,policy);}catch(e){if(e.status===404)return null;throw e;}
  if(s.state!=='active')return null;
  const boardId=String(s.originBoardId),field=await boardEstimateField(boardId,policy);
  const entry={id:String(id),name:s.name,state:s.state,startDate:s.startDate,startMs:Date.parse(s.startDate),boardId,estimateFieldId:field,fieldHistory:[{at:0,fieldId:field}]};
  if(s.state==='closed')entry.closeMs=Date.parse(s.completeDate??s.endDate);
  cfg.sprints[id]=entry;return entry;
}
