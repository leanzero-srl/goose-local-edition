import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {createRequire} from 'node:module';
const require=createRequire(import.meta.url);
const {chromium}=require(process.env.GOOSE_SWARM_PLAYWRIGHT_MODULE);
const source=readFileSync(new URL('./product_probe_sb71.mjs',import.meta.url),'utf8');
const helper=source.slice(source.indexOf('function pageSyncTruth('),source.indexOf('\nfunction pageSyncState()'));
const records=[
  {id:'eur',currency:'EUR',amount_minor:1234,status:'settled',note:'retained',text:'EUR 12.34'},
  {id:'usd',currency:'USD',amount_minor:12345,status:'pending',note:'',text:'$123.45'},
  {id:'jpy',currency:'JPY',amount_minor:9876543,status:'refunded',note:'',text:'JPY 9,876,543'},
  {id:'kwd',currency:'KWD',amount_minor:9876543,status:'failed',note:'',text:'KWD 9,876.543'},
];
const expected={payments:{data:records},summary:{by_currency:records.map(r=>({currency:r.currency,count:1,total_minor:r.amount_minor}))}};
const html=`<table id="a-valid-different-id"><tbody>${records.map(r=>`<tr data-id="${r.id}"><td>20 Sep 2026</td><td>${r.text}</td><td>${r.status}</td><td>Ada</td><td>${r.note||'—'}</td></tr>`).join('')}</tbody></table>`
  +records.map(r=>`<div class="cur-total" data-currency="${r.currency}"><div>${r.currency}</div><div>1</div><div>${r.text}</div></div>`).join('');
const browser=await chromium.launch({headless:true,executablePath:process.env.GOOSE_SWARM_CHROMIUM_EXECUTABLE});
try {
  const page=await browser.newPage();
  const read=()=>page.evaluate(({helper,expected})=>(0,eval)('('+helper+')')(expected),{helper,expected});
  await page.setContent(html);
  assert.equal((await read()).ok,true,'correct financial truth with no mandated table ID');
  const mutations=[
    ['shifted decimal',()=>document.querySelector('tr[data-id="eur"] td:nth-child(2)').textContent='EUR 1234'],
    ['wrong currency',()=>document.querySelector('tr[data-id="eur"] td:nth-child(2)').textContent='USD 12.34'],
    ['wrong sign',()=>document.querySelector('tr[data-id="eur"] td:nth-child(2)').textContent='EUR -12.34'],
    ['incorrect status',()=>document.querySelector('tr[data-id="eur"] td:nth-child(3)').textContent='not settled'],
    ['duplicate identity',()=>document.querySelector('tr[data-id="usd"]').replaceWith(document.querySelector('tr[data-id="eur"]').cloneNode(true))],
    ['stale note',()=>document.querySelector('tr[data-id="eur"] td:nth-child(5)').textContent='stale'],
    ['hidden table',()=>document.querySelector('table').style.display='none'],
    ['summary amount',()=>document.querySelector('.cur-total[data-currency="EUR"]').lastElementChild.textContent='EUR 1234'],
    ['summary count',()=>document.querySelector('.cur-total[data-currency="EUR"]').children[1].textContent='2'],
  ];
  for(const [name,mutate] of mutations){await page.setContent(html);await page.evaluate(mutate);assert.equal((await read()).ok,false,name);}
  console.log('PASS: correct no-op truth and nine actual-DOM corruption controls');
} finally {await browser.close();}
