// Real-browser control for the drag-budget read (run 43896353, scored in-app: c1 null, four rows
// "not measurable"). Run with the bundled runtime:
//   GOOSE_SWARM_PLAYWRIGHT_MODULE=... GOOSE_SWARM_CHROMIUM_EXECUTABLE=... node test_sb71_drag_budget_read.mjs
// The page draws on demand, one heavy software-GL frame per pointermove, the shape of that run's
// app: the main thread is idle while the GPU process finishes the frame, so the rAF that takes c1
// lands later than a fixed post-release delay. The control first proves the old fixed-delay read
// misses c1 on this page, then that the probe's read waits for it, and that a page whose rAF never
// fires still returns (bounded) with c1 absent. Both probes carry the read (SB7.1 and sb-7).
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {createRequire} from 'node:module';
import {createServer} from 'node:http';
const require=createRequire(import.meta.url),{chromium}=require(process.env.GOOSE_SWARM_PLAYWRIGHT_MODULE);
function probeHelpers(file){
  const source=readFileSync(new URL('./'+file,import.meta.url),'utf8');
  const start=source.indexOf('function pageArmBudgetWatch('),end=source.indexOf('// §3.4 coast sampler');
  assert.ok(start>0&&end>start,'budget watch helpers not found in '+file);
  const launchArgsAt=source.indexOf('const VIZ_LAUNCH_ARGS');
  return {...(0,eval)('(()=>{'+source.slice(start,end)+
    ';return {pageArmBudgetWatch,pageReadBudgetWatch,readBudgetWatchAfterRelease};})()'),
    VIZ_LAUNCH_ARGS:(0,eval)(source.slice(source.indexOf('[',launchArgsAt),source.indexOf('];',launchArgsAt)+1))};
}
const PROBES=['product_probe_sb71.mjs','product_probe_v3.mjs'].map(file=>({file,...probeHelpers(file)}));
assert.deepEqual(PROBES[0].VIZ_LAUNCH_ARGS,PROBES[1].VIZ_LAUNCH_ARGS);
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const RACE_ATTEMPTS=6;

const page_html=`<!doctype html><body style="margin:0"><canvas id="c" width="1280" height="800"></canvas><script>
const gl=document.getElementById('c').getContext('webgl2');
let iterations=+new URLSearchParams(location.search).get('iterations');
const shader=(type,text)=>{const s=gl.createShader(type);gl.shaderSource(s,text);gl.compileShader(s);return s;};
const program=gl.createProgram();
gl.attachShader(program,shader(gl.VERTEX_SHADER,'#version 300 es\\nin vec2 p;void main(){gl_Position=vec4(p,0,1);}'));
gl.attachShader(program,shader(gl.FRAGMENT_SHADER,'#version 300 es\\nprecision highp float;uniform int n;uniform float k;out vec4 o;void main(){float a=gl_FragCoord.x*k;for(int i=0;i<n;i++){a=sin(a)*1.0001+cos(a*0.999);}o=vec4(fract(a),0.2,0.4,1);}'));
gl.linkProgram(program);gl.useProgram(program);
const buffer=gl.createBuffer();gl.bindBuffer(gl.ARRAY_BUFFER,buffer);
gl.bufferData(gl.ARRAY_BUFFER,new Float32Array([-1,-1,1,-1,-1,1,1,1]),gl.STATIC_DRAW);
gl.enableVertexAttribArray(0);gl.vertexAttribPointer(0,2,gl.FLOAT,false,0,0);
window.__p7={defDraws:0,offDraws:0};let frames=0,pending=false,k=0.001;
window.vs7dbg={frames:()=>frames};
const draw=()=>{pending=false;k+=0.0001;gl.uniform1i(gl.getUniformLocation(program,'n'),iterations);
  gl.uniform1f(gl.getUniformLocation(program,'k'),k);gl.drawArrays(gl.TRIANGLE_STRIP,0,4);window.__p7.defDraws++;frames++;};
const invalidate=()=>{if(!pending){pending=true;requestAnimationFrame(draw);}};
window.addEventListener('pointermove',invalidate);
window.__setIterations=n=>{iterations=n;};
window.__frameMs=()=>new Promise(resolve=>{const t0=performance.now();invalidate();requestAnimationFrame(()=>{invalidate();requestAnimationFrame(()=>resolve(performance.now()-t0));});});
</script>`;
const server=createServer((req,res)=>{res.setHeader('Content-Type','text/html');res.end(page_html);});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
const base='http://127.0.0.1:'+server.address().port;
const browser=await chromium.launch({headless:true,executablePath:process.env.GOOSE_SWARM_CHROMIUM_EXECUTABLE,args:PROBES[0].VIZ_LAUNCH_ARGS});

async function drag(page,{pageArmBudgetWatch}){
  await page.evaluate(pageArmBudgetWatch);
  await page.mouse.move(200,200);await page.mouse.down();
  for(let step=1;step<=6;step++){await page.mouse.move(200+4*step,200,{steps:1});await sleep(20);}
  await page.mouse.up();
  await sleep(300);
}
try{
  const page=await browser.newPage({viewport:{width:1280,height:800}});
  // Calibrate the frame cost on THIS machine until one frame clearly outlasts the 300 ms read.
  let iterations=64,frameMs=0;
  await page.goto(base+'/?iterations='+iterations);
  while(frameMs<900&&iterations<1<<20){
    iterations*=2;await page.evaluate(n=>window.__setIterations(n),iterations);
    frameMs=await page.evaluate(()=>window.__frameMs());
  }
  console.log(JSON.stringify({iterations,frameMs}));
  assert.ok(frameMs>=900,'could not make a frame outlast the fixed read on this machine');

  for(const probe of PROBES){
    // Whether pointerup lands while a frame is still in the GPU process depends on Chrome's frame
    // scheduling, so the race is retried; every attempt must end with c1 in hand.
    let reproduced=0;
    for(let attempt=1;attempt<=RACE_ATTEMPTS&&!reproduced;attempt++){
      await page.goto(base+'/?iterations='+iterations);await page.evaluate(()=>window.__frameMs());
      await drag(page,probe);
      const fixedDelayRead=await page.evaluate(probe.pageReadBudgetWatch);
      assert.ok(fixedDelayRead.c0&&fixedDelayRead.cUp,JSON.stringify(fixedDelayRead));
      const waited=await probe.readBudgetWatchAfterRelease(page);
      assert.ok(waited.c1,'the probe read returned without c1: '+JSON.stringify(waited));
      if(fixedDelayRead.c1===null){
        reproduced=attempt;
        assert.equal(waited.c1Wait.landed,true,JSON.stringify(waited.c1Wait));
        assert.ok(waited.c1.t-waited.cUp.t>300,'the late sample was not late: '+JSON.stringify(waited));
      }
      console.log(JSON.stringify({probe:probe.file,attempt,fixedDelayReadC1:fixedDelayRead.c1,c1:waited.c1,c1Wait:waited.c1Wait??null}));
    }
    assert.ok(reproduced,'the fixed-delay race did not reproduce in '+RACE_ATTEMPTS+' attempts');

    await page.goto(base+'/?iterations=1');
    await drag(page,probe);
    const fast=await probe.readBudgetWatchAfterRelease(page);
    assert.ok(fast.c1&&!('c1Wait' in fast),'a frame within the read must be returned untouched: '+JSON.stringify(fast));

    await page.goto(base+'/?iterations=1');
    await page.evaluate(()=>{window.requestAnimationFrame=()=>0;});
    await drag(page,probe);
    const started=Date.now();
    const never=await probe.readBudgetWatchAfterRelease(page);
    assert.equal(never.c1,null,JSON.stringify(never));
    assert.equal(never.c1Wait.landed,false,JSON.stringify(never.c1Wait));
    assert.ok(Date.now()-started<never.c1Wait.boundMs+5000,'the wait is not bounded: '+JSON.stringify(never.c1Wait));
    console.log(JSON.stringify({probe:probe.file,never:never.c1Wait}));
  }
}finally{await browser.close();await new Promise(resolve=>server.close(resolve));}
