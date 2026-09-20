import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {createRequire} from 'node:module';
import {createServer} from 'node:http';
const require=createRequire(import.meta.url),{chromium}=require(process.env.GOOSE_SWARM_PLAYWRIGHT_MODULE);
const source=readFileSync(new URL('./product_probe_sb71.mjs',import.meta.url),'utf8');
const helpers=source.slice(source.indexOf('function workflowPayloadMatches('),source.indexOf('async function flowScenario('));
const money=source.slice(source.indexOf('function parseVisibleMoney('),source.indexOf('function parseVisibleVersion('));
const measure=new Function('sleep',money+helpers+';return measureWorkflowPayment;')(ms=>new Promise(r=>setTimeout(r,ms)));
const target={id:'new-payment',currency:'EUR',amount_minor:1234,status:'pending',note:'unique F1',counterparty:{name:'Ada',country:'DE'}};
let mode='normal';
let rows=Array.from({length:5},(_,i)=>({...target,id:'old-'+i,note:'old'}));rows.push(target);
const server=createServer((req,res)=>{
 const u=new URL(req.url,'http://localhost');res.setHeader('Content-Type','application/json');
 if(u.pathname==='/api/payments/new-payment'){res.statusCode=mode==='missing-ledger'?404:200;res.end(JSON.stringify(mode==='missing-ledger'?{}:target));return;}
 if(u.pathname==='/api/payments'){const ordered=u.searchParams.get('sort')==='-created_at'?[...rows].reverse():rows;const offset=Number(u.searchParams.get('offset')||0);res.end(JSON.stringify({total:rows.length,data:ordered.slice(offset,offset+Number(u.searchParams.get('limit')||2))}));return;}
 res.setHeader('Content-Type','text/html');res.end(`<table><thead><tr><th id=date>Date</th></tr></thead><tbody></tbody></table><button id=prev>Prev</button><button id=next>Next</button><script>
 let offset=0,sort='created_at';const mode=${JSON.stringify(mode)},pageSize=['far','midrank'].includes(mode)?50:2;
 async function load(){const d=await fetch('/api/payments?limit='+pageSize+'&offset='+offset+'&sort='+sort).then(r=>r.json());document.querySelector('tbody').innerHTML=d.data.map(r=>'<tr data-id="'+(mode==='wrong-id'&&r.id==='new-payment'?'impostor':r.id)+'"><td>Date</td><td>'+(mode==='wrong-money'&&r.id==='new-payment'?'EUR99.00':'EUR12.34')+'</td><td>pending</td><td>Ada</td><td>'+r.note+'</td></tr>').join('');prev.disabled=offset===0;next.disabled=offset+pageSize>=d.total;}
 date.onclick=()=>{if(mode==='unrelated'){document.querySelector('td').textContent='changed';return;}sort=sort==='created_at'?'-created_at':'created_at';offset=0;load();};next.onclick=()=>{if(mode==='unrelated'){document.querySelector('td').textContent='changed';return;}offset+=pageSize;load();};prev.onclick=()=>{offset-=pageSize;load();};load();</script>`);
});
await new Promise(r=>server.listen(0,'127.0.0.1',r));const base='http://127.0.0.1:'+server.address().port;
const browser=await chromium.launch({headless:true,executablePath:process.env.GOOSE_SWARM_CHROMIUM_EXECUTABLE});
try{
 const page=await browser.newPage();
 for(mode of ['normal','wrong-id','wrong-money','unrelated','missing-ledger']){
  await page.goto(base);await page.waitForSelector('tr[data-id]');
  const result=await measure(page,base,target,()=>({rows:[target]}));
  assert.equal(result.ok,mode==='normal',JSON.stringify(result));
  if(mode==='normal'){assert.equal(result.navigation[0].action,'Date sort');assert.equal(result.rendered.id,target.id);}
  console.log(JSON.stringify({mode,ok:result.ok,reason:result.reason,navigation:result.navigation.length}));
 }
 mode='far';rows=Array.from({length:12288},(_,i)=>({...target,id:'old-'+i,note:'old'}));rows.push(target);
 await page.goto(base);await page.waitForSelector('tr[data-id]');const started=Date.now();const far=await measure(page,base,target,()=>({rows:[target]}));assert.equal(far.ok,true,JSON.stringify(far));assert.equal(far.navigation.length,1);console.log(JSON.stringify({mode:'far',rows:rows.length,milliseconds:Date.now()-started,navigation:far.navigation}));
 mode='midrank';rows=Array.from({length:12288},(_,i)=>({...target,id:'old-'+i,note:'old'}));rows.splice(6144,0,target);await page.goto(base);await page.waitForSelector('tr[data-id]');const midStart=Date.now();const middle=await measure(page,base,target,()=>({rows:[target]}));assert.equal(middle.ok,true,JSON.stringify(middle));assert.equal(middle.navigation.length,122);console.log(JSON.stringify({mode,rows:rows.length,milliseconds:Date.now()-midStart,navigation:middle.navigation.length}));
 const ambiguous=await measure(page,base,target,()=>({rows:[target,{...target,id:'duplicate'}]}));assert.ok(ambiguous.unavailable);
}finally{await browser.close();await new Promise(r=>server.close(r));}
