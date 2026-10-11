import { createHmac,timingSafeEqual } from 'node:crypto';
import { kvs } from '@forge/kvs';
import { enqueue } from './sync';
export async function ciDeployment(request){
 const unauthorized={outputKey:'unauthorized'};
 const headers=Object.fromEntries(Object.entries(request?.headers??{}).map(([k,v])=>[k.toLowerCase(),Array.isArray(v)?v[0]:v]));
 const secret=await kvs.getSecret('ci-secret'),timestamp=headers['x-lz-timestamp'],signature=headers['x-lz-signature'];
 if(!secret||typeof request.body!=='string'||typeof signature!=='string'||!/^sha256=[0-9a-f]{64}$/i.test(signature)||typeof timestamp!=='string')return unauthorized;
 const expected=createHmac('sha256',secret).update(`${timestamp}.${request.body}`).digest(),actual=Buffer.from(signature.slice(7),'hex');
 if(actual.length!==expected.length||!timingSafeEqual(actual,expected))return unauthorized;
 const at=Number(timestamp);if(!Number.isFinite(at)||Math.abs(Date.now()/1000-at)>300)return unauthorized;
 let body;try{body=JSON.parse(request.body);}catch{return unauthorized;}
 if(!body||typeof body.eventId!=='string'||!body.eventId||!['staging','production'].includes(body.environment)||!Number.isFinite(body.sentAt)||!Array.isArray(body.issueKeys)||body.issueKeys.some(k=>typeof k!=='string'||!k))return unauthorized;
 const entity=kvs.entity('ci-deployment');
 try{await entity.set(body.eventId,{eventId:body.eventId,at:Date.now(),environment:body.environment,issueKeys:[...new Set(body.issueKeys)].join(','),state:'pending'},{keyPolicy:'FAIL_IF_EXISTS'});}catch(e){if(await entity.get(body.eventId))return {outputKey:'duplicate'};throw e;}
 await enqueue({kind:'deployment',eventId:body.eventId});return {outputKey:'accepted'};
}
