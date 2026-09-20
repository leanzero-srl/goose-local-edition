import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {createRequire} from 'node:module';
import {createServer} from 'node:http';
const require=createRequire(import.meta.url),{chromium}=require(process.env.GOOSE_SWARM_PLAYWRIGHT_MODULE);
const source=readFileSync(new URL('./product_probe_sb71.mjs',import.meta.url),'utf8');
const helper=source.slice(source.indexOf('async function measureOptimisticNote('),source.indexOf('async function flowScenario('));
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const measure=(0,eval)('('+helper.trim()+')');
globalThis.sleep=sleep;
let record,currentMode,reads;
const server=createServer(async(req,res)=>{
 if(req.url==='/api/payments/p1/note'){
  let body='';for await(const chunk of req)body+=chunk;
  record={...record,note:JSON.parse(body).note,version:record.version+1};res.setHeader('Content-Type','application/json');res.end(JSON.stringify(record));return;
 }
 if(req.url==='/api/payments/p1'){reads++;if(currentMode==='interference'&&reads===2)record={...record,version:record.version+1};res.setHeader('Content-Type','application/json');res.end(JSON.stringify(record));return;}
 const mode=new URL(req.url,'http://localhost').searchParams.get('mode');currentMode=mode;reads=0;
 res.setHeader('Content-Type','text/html');let html=`<table><tbody><tr data-id="p1"><td>Date</td><td>EUR1.00</td><td>settled</td><td>Ada</td><td id="note">old note</td></tr></tbody></table><script>
 const cell=document.getElementById('note'),row=cell.parentElement;
 cell.onclick=()=>{if(cell.querySelector('input'))return;cell.innerHTML='<input value="old note"><button>Save</button>';cell.querySelector('button').onclick=async event=>{
 event.stopPropagation();const value=cell.querySelector('input').value;
 const paint=()=>{cell.textContent=value;row.dataset.state='saving';};
 if(${JSON.stringify(mode)}==='normal'||${JSON.stringify(mode)}==='aria'||${JSON.stringify(mode)}==='interference')paint();if(${JSON.stringify(mode)}==='delayed')setTimeout(paint,180);
 const data=await fetch('/api/payments/p1/note',{method:'POST',body:JSON.stringify({note:value})}).then(r=>r.json());cell.textContent=data.note;row.dataset.state='saved';};};
 </script>`;
 if(mode==='aria')html=html.replace('<table><tbody>','<div role=table>').replace('</tbody></table>','</div>').replace('<tr data-id="p1">','<div role=row data-id="p1">').replace('</tr>','</div>').replaceAll('<td>','<div role=cell>').replace('<td id="note">','<div role=cell id="note">').replaceAll('</td>','</div>');
 res.end(html);
});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
const base='http://127.0.0.1:'+server.address().port;
const browser=await chromium.launch({headless:true,executablePath:process.env.GOOSE_SWARM_CHROMIUM_EXECUTABLE});
try{
 const page=await browser.newPage();
 for(const mode of ['normal','delayed','network-only','aria','interference']){
  record={id:'p1',note:'old note',version:1};await page.goto(base+'/?mode='+mode);
  const result=await measure(page,base,{},async()=>{});
  assert.equal(result.backendUnchangedWhileHeld,mode!=='interference',mode);assert.equal(result.savedAfterRelease,true,mode);
  assert.equal(result.paintedWhileHeld,!['network-only','interference'].includes(mode),JSON.stringify(result));
  if(mode==='normal'||mode==='aria')assert.ok(result.paintMs<100,JSON.stringify(result));
  if(mode==='delayed')assert.ok(result.paintMs>=180,JSON.stringify(result));
  console.log(JSON.stringify({mode,paintMs:result.paintMs,held:result.heldDuringCheck,painted:result.paintedWhileHeld,saved:result.savedAfterRelease}));
 }
}finally{await browser.close();await new Promise(resolve=>server.close(resolve));}
