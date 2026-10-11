import Resolver from '@forge/resolver';
import { InvocationError,InvocationErrorCode } from '@forge/events';
import { loadConfig } from './config';
import { RateLimited } from './jira';
import { enqueue,background,estimateFieldIds,applyIssueEvent,reconcileBatch,migrateOnly,applyPendingDeployments } from './sync';
import { refreshPrecomputations } from './search';
import { boardsView,widgetView,getSprint,personView,postSummary } from './views';
import { explainSprint } from './explain';
import { CHANNEL } from './realtime';
import { authorize,adminView,saveSettings,rotateSecret } from './admin';
export { ciDeployment } from './ci';
export { scopeChanged } from './search';
export async function onIssueUpdated(event){
 const id=event?.issue?.id;if(!id)return;
 if(event.eventType==='avi:jira:deleted:issue'||event.eventType==='issue_deleted'){await enqueue({issueId:String(id),deleted:true});return;}
 const cfg=await loadConfig(),items=event.changelog?.items??[],fields=new Set(cfg?estimateFieldIds(cfg):[]);
 const relevant=items.some(i=>i.fieldId===cfg?.sprintFieldId||i.field==='Sprint'||fields.has(i.fieldId)||fields.has(i.field));
 if(relevant)await enqueue({issueId:String(id),changelogId:String(event.changelog?.id??'')});
}
export async function onIssueDeleted(event){if(event.issue?.id)await enqueue({issueId:String(event.issue.id),deleted:true});}
export async function reconcile(){await enqueue({kind:'reconcile'});}
export async function consumeChange(event){
 try{
  await refreshPrecomputations(background);
  const body=event.body;let result;
  if(body.kind==='reconcile')result=await reconcileBatch();
  else if(body.kind==='migration')result=await migrateOnly();
  else if(body.kind==='deployment'){const cfg=await loadConfig();if(!cfg){await enqueue({kind:'reconcile'});return;}await applyPendingDeployments(cfg,body.eventId);result={ok:true};}
  else result=await applyIssueEvent(body);
  await refreshPrecomputations(background);return result;
 }catch(e){if(e instanceof RateLimited)return new InvocationError({retryAfter:Math.min(900,e.retryAfterSeconds),retryReason:InvocationErrorCode.FUNCTION_UPSTREAM_RATE_LIMITED,retryData:{reason:e.reason,seconds:e.retryAfterSeconds}});throw e;}
}
const resolver=new Resolver();
const safe=fn=>async req=>{try{return await fn(req);}catch(e){if(e instanceof RateLimited)return {rateLimited:true,retryAfter:e.retryAfterSeconds};return {ok:false,error:e.message};}};
const sprintId=ctx=>ctx?.extension?.sprint?.id==null?null:String(ctx.extension.sprint.id);
const getView=async ctx=>{const id=sprintId(ctx);if(!id)throw new Error('This action was opened without a sprint.');const s=await getSprint(id,{});if(!s)throw new Error('The sprint was not found.');return s.started?personView(s,{}):{notStarted:true,sprint:s};};
resolver.define('boards',safe(async()=>({boards:await boardsView({})})));
resolver.define('widget',safe(async({payload,context})=>{const id=payload?.boardId??context.extension?.config?.boardId;if(id==null||id==='')return {needsConfig:true};return {...await widgetView(String(id),{}),realtime:{channel:CHANNEL}};}));
resolver.define('sprintLedger',safe(({context})=>getView(context)));
resolver.define('explain',safe(async({context})=>{const view=await getView(context);if(view.notStarted)return {ok:false,error:'This sprint has not started.'};return explainSprint(view);}));
resolver.define('postSummary',safe(async({payload,context})=>{const id=sprintId(context),s=id&&await getSprint(id,{});if(!s)throw new Error('The sprint was not found.');const requestId=payload?.requestId??`${id}:${payload?.changeId}`;return postSummary(s,payload?.changeId,{},context,requestId);}));
const admin=fn=>safe(async req=>{await authorize();return fn(req);});
resolver.define('adminLoad',admin(()=>adminView()));
resolver.define('adminSave',admin(({payload,context})=>saveSettings(payload,context)));
resolver.define('adminRotate',admin(({context})=>rotateSecret(context)));
export const uiResolver=resolver.getDefinitions();
export async function getSprintScope(payload){
 try{const id=String(payload?.sprintId??'').trim();if(!/^\d+$/.test(id))return {error:'sprintId must be the numeric id of a sprint.'};const s=await getSprint(id,{});if(!s)return {error:'The sprint was not found.'};const v=await personView(s,{});return {sprintId:s.id,sprintName:s.name,...v.values,hiddenChanges:v.hiddenCount,changes:v.changes.map(c=>({changeId:c.changeId,issueKey:c.issueKey,kind:c.kind,points:c.points,at:c.at,by:c.by}))};}catch(e){return {error:e.message};}
}
