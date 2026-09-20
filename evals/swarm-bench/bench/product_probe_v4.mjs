/** SB-8 observes real browser pixels and input, never a candidate's self-reported scene graph. */
import { createRequire } from 'node:module';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
const require=createRequire(import.meta.url);
let chromium;
try { ({chromium}=require(process.env.GOOSE_SWARM_PLAYWRIGHT_MODULE || 'playwright')); }
catch { ({chromium}=require(path.join(execFileSync('npm',['root','-g'],{encoding:'utf8'}).trim(),'playwright'))); }
if(process.argv.includes('--preflight')){const b=await chromium.launch({headless:true,...(process.env.GOOSE_SWARM_CHROMIUM_EXECUTABLE?{executablePath:process.env.GOOSE_SWARM_CHROMIUM_EXECUTABLE}:{})});await b.close();console.log(JSON.stringify({ok:true}));process.exit(0)}
const base=process.argv.find(a=>a.startsWith('--base='))?.slice(7)??(process.argv[2]==='load'?process.argv[3]:process.argv[2]);
const shots=process.env.BENCH_SHOTS_DIR;
const rows=[];const check=(name,tier,score,detail='')=>rows.push({name,tier,score:Number(score),detail});
function png(buf){let i=8,w,h,channels,parts=[];while(i<buf.length){let n=buf.readUInt32BE(i),type=buf.toString('ascii',i+4,i+8),data=buf.subarray(i+8,i+8+n);if(type==='IHDR'){w=data.readUInt32BE(0);h=data.readUInt32BE(4);if(data[8]!==8||![2,6].includes(data[9]))throw Error('unsupported probe PNG');channels=data[9]===6?4:3}if(type==='IDAT')parts.push(data);i+=n+12}let raw=zlib.inflateSync(Buffer.concat(parts)),stride=w*channels,out=Buffer.alloc(stride*h);for(let y=0;y<h;y++){let type=raw[y*(stride+1)];for(let x=0;x<stride;x++){let a=x>=channels?out[y*stride+x-channels]:0,b=y?out[(y-1)*stride+x]:0,c=y&&x>=channels?out[(y-1)*stride+x-channels]:0,p=a+b-c,pa=Math.abs(p-a),pb=Math.abs(p-b),pc=Math.abs(p-c);let add=[0,a,b,Math.floor((a+b)/2),pa<=pb&&pa<=pc?a:pb<=pc?b:c][type];out[y*stride+x]=(raw[y*(stride+1)+1+x]+add)&255}}return {w,h,at:(x,y)=>out.subarray((Math.floor(y)*w+Math.floor(x))*channels,(Math.floor(y)*w+Math.floor(x))*channels+3)}}
const rgb=hex=>[1,3,5].map(i=>parseInt(hex.slice(i,i+2),16));
function pixels(im,color){const c=rgb(color),pts=[];for(let y=0;y<im.h;y++)for(let x=0;x<im.w;x++){let p=im.at(x,y);if(c.every((v,k)=>Math.abs(v-p[k])<16))pts.push([x,y])}return pts}
function near(im,point,color,r=4){const c=rgb(color);for(let y=Math.max(0,Math.round(point[1])-r);y<=Math.min(im.h-1,Math.round(point[1])+r);y++)for(let x=Math.max(0,Math.round(point[0])-r);x<=Math.min(im.w-1,Math.round(point[0])+r);x++){let p=im.at(x,y);if(c.every((v,k)=>Math.abs(v-p[k])<18))return true}return false}
function project(sc,im,mode,point){const target=[sc.width/2,mode==='top'?0:sc.height/2,sc.depth/2],eye=mode==='front'?[0,0,1]:mode==='top'?[0,1,0]:[20,20,24],up=mode==='top'?[0,0,-1]:[0,1,0];const unit=a=>{let n=Math.hypot(...a);return a.map(x=>x/n)},cross=(a,b)=>[a[1]*b[2]-a[2]*b[1],a[2]*b[0]-a[0]*b[2],a[0]*b[1]-a[1]*b[0]],dot=(a,b)=>a.reduce((s,x,i)=>s+x*b[i],0);let z=unit(eye),x=unit(cross(up,z)),y=cross(z,x),d=point.map((v,i)=>v-target[i]),aspect=im.w/im.h,span=mode==='front'?Math.max(sc.height+4,(sc.width+4)/aspect):mode==='top'?Math.max(sc.depth+4,(sc.width+4)/aspect):1.6*Math.max(sc.width,sc.depth,sc.height);return [im.w/2+dot(d,x)*im.h/span,im.h/2-dot(d,y)*im.h/span]}
const browser=await chromium.launch({headless:true,...(process.env.GOOSE_SWARM_CHROMIUM_EXECUTABLE?{executablePath:process.env.GOOSE_SWARM_CHROMIUM_EXECUTABLE}:{})});const page=await browser.newPage({viewport:{width:1280,height:900},deviceScaleFactor:1});page.setDefaultTimeout(5000);let errors=[];page.on('pageerror',e=>errors.push(e.message));
await page.addInitScript(()=>{window.__probeCanvases=new Map();const original=HTMLCanvasElement.prototype.getContext;HTMLCanvasElement.prototype.getContext=function(...args){let ctx=original.apply(this,args);if(ctx&&String(args[0]).includes('webgl')){if(!window.__probeCanvases.has(this))window.__probeCanvases.set(this,{contexts:0,draws:0});const counters=window.__probeCanvases.get(this);counters.contexts++;for(let name of ['drawArrays','drawElements','drawArraysInstanced','drawElementsInstanced'])if(ctx[name]&&!ctx[name].__probed){let fn=ctx[name].bind(ctx);let wrapped=(...a)=>{counters.draws++;return fn(...a)};wrapped.__probed=true;ctx[name]=wrapped}}return ctx}});
try{
await page.goto(base,{waitUntil:'networkidle',timeout:15000});const canvas=page.getByTestId('scene');await canvas.waitFor({timeout:7000});const sc=await(await fetch(base+'/api/scene')).json();let state=await(await fetch(base+'/api/state')).json();
async function shot(mode){await page.getByTestId('camera-'+mode).click();await page.waitForTimeout(180);const bytes=await canvas.screenshot();if(shots){fs.mkdirSync(shots,{recursive:true});fs.writeFileSync(path.join(shots,'sb8-'+mode+'.png'),bytes)}return png(bytes)}

if(process.argv[2]==='load'){
  await page.waitForFunction(() => (window.__probeCanvases.get(document.querySelector('[data-testid=scene]'))?.draws??0)>0,{timeout:5000}).catch(()=>{});
  const renderedRowCount=await page.locator('[data-testid^="box-"]').filter({visible:true}).count();
  const gl=await page.evaluate(()=>window.__probeCanvases.get(document.querySelector('[data-testid=scene]')));
  if(!gl?.draws)errors.push('Scene canvas renders no WebGL geometry');
  if(shots){fs.mkdirSync(shots,{recursive:true});await canvas.screenshot({path:path.join(shots,`sb8-gate-${Date.now()}.png`)});}
  console.log(JSON.stringify({renderedRowCount,totalClaimedInDom:state.boxes.length,consoleErrors:{count:errors.length,texts:errors}}));
  await browser.close();process.exit(0);
}
await page.waitForFunction(() => (window.__probeCanvases.get(document.querySelector('[data-testid=scene]'))?.draws ?? 0) > 0, { timeout: 5000 }).catch(() => {});
const top=await shot('top');const front=await shot('front');
const size=await canvas.boundingBox();check('scene_canvas_size','D',size.width>=600&&size.height>=400,`${size.width}x${size.height} CSS pixels`);
const gl=await page.evaluate(()=>window.__probeCanvases.get(document.querySelector('[data-testid=scene]'))??{contexts:0,draws:0});check('webgl_geometry','C',gl.contexts>0&&gl.draws>5,JSON.stringify(gl));
let accurate=0,total=0;
for(let b of state.boxes){for(let [mode,im] of [['top',top],['front',front]]){const pts=pixels(im,b.color);if(mode==='front'&&state.boxes.some(o=>o.id!==b.id&&o.z>b.z&&Math.abs(o.x-b.x)<(o.w+b.w)/2))continue;total++;let expected=project(sc,im,mode,[b.x,b.y+b.h/2,b.z]);let found=pts.some(([x,y])=>Math.hypot(x-expected[0],y-expected[1])<12);let span=mode==='top'?Math.max(sc.depth+4,(sc.width+4)/(im.w/im.h)):Math.max(sc.height+4,(sc.width+4)/(im.w/im.h));let area=b.w*(mode==='top'?b.d:b.h)*(im.h/span)**2;accurate+=Number(found&&pts.length>area*.35&&pts.length<area*1.5)}}
check('seeded_box_geometry','C',accurate/total,`${accurate}/${total} projected boxes; sizes ${top.w}x${top.h}`);
const iso=await shot('iso');const p=state.pose,load=state.boxes.find(b=>b.id===state.held),sy=p.y+(load?.h??0)+.3;
const structure=[['columns_rails','#475569',[[0,sc.height/2,0],[sc.width,sc.height/2,0],[0,sc.height/2,sc.depth],[sc.width,sc.height/2,sc.depth],[0,sc.height,sc.depth/2],[sc.width,sc.height,sc.depth/2]]],['bridge_beams','#f59e0b',[[2,sc.height,p.z-.25],[10,sc.height,p.z+.25]]],['trolley_spreader','#06b6d4',[[p.x,sc.height+.35,p.z]]],['four_cables','#e2e8f0',[-.4,.4].flatMap(dx=>[-.3,.3].map(dz=>[p.x+dx,(sy+sc.height+.2)/2,p.z+dz]))],['spreader','#ef4444',[[p.x,sy,p.z]]],['wheels','#111827',[-.6,.6].flatMap(dx=>[-.5,.5].map(dz=>[p.x+dx,sc.height+.05,p.z+dz]))]];
for(let [name,color,points] of structure){check(name,'C',points.filter(pt=>[['iso',iso],['top',top],['front',front]].some(([mode,im])=>near(im,project(sc,im,mode,pt),color))).length/points.length)}
const b=state.boxes[0];
async function scenario(name,body){try{await body()}catch(e){errors.push(name+': '+String(e))}}
await scenario('3D selection',async()=>{
await page.getByTestId('camera-top').click();let pt=project(sc,top,'top',[b.x,b.y+b.h,b.z]),rect=await canvas.boundingBox();await page.mouse.click(rect.x+pt[0],rect.y+pt[1]);check('real_3d_pick','D',(await page.getByTestId('selection').innerText()).includes(b.id));
});
await scenario('table selection',async()=>{
await page.getByTestId('box-'+state.boxes[1].id).click();check('table_selection','D',(await page.getByTestId('selection').innerText()).includes(state.boxes[1].id));
});
await scenario('UI move and rejection',async()=>{
const target={x:3.7,z:7.3,y:5.2,yaw:30};for(let [key,label] of [['x','X'],['z','Z'],['y','Height'],['yaw','Yaw']])await page.getByRole('spinbutton',{name:label,exact:true}).fill(String(target[key]));await page.getByRole('button',{name:'Move',exact:true}).click();await page.waitForTimeout(1100);let after=await(await fetch(base+'/api/state')).json();check('ui_move_reaches_backend','D',Object.entries(target).every(([k,v])=>Math.abs(after.pose[k]-v)<1e-6));
let moved=await shot('iso');check('crane_tracks_state','C',near(moved,project(sc,moved,'iso',[target.x,sc.height+.35,target.z]),'#06b6d4')&& !near(moved,project(sc,moved,'iso',[p.x,sc.height+.35,p.z]),'#06b6d4'));
const messages=page.locator('[role=alert]:visible, [role=status]:visible');
const priorMessage=(await messages.allTextContents()).join(' ').trim();
await page.getByRole('spinbutton',{name:'X',exact:true}).fill('-10');await page.getByRole('button',{name:'Move',exact:true}).click();await page.waitForTimeout(400);const errorMessage=(await messages.allTextContents()).join(' ').trim();check('visible_command_error','D',errorMessage.length>0&&errorMessage!==priorMessage,errorMessage);check('invalid_ui_move_is_atomic','D',(await(await fetch(base+'/api/state')).json()).revision===after.revision);
});
// External mutations must reach actual pixels, not just a revision counter.
async function external(op,values){let current=await(await fetch(base+'/api/state')).json();let r=await fetch(base+'/api/commands',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({id:crypto.randomUUID(),revision:current.revision,op,...values})});if(!r.ok)throw Error('setup '+await r.text());return r.json()}
await scenario('external state and load controls',async()=>{
await external('move',{x:b.x,z:b.z,y:b.y+b.h,yaw:0});await external('grip',{boxId:b.id});await external('move',{x:b.x,z:b.z,y:5,yaw:0});await page.waitForTimeout(1400);let lifted=await shot('front');check('cargo_tracks_external_backend','C',near(lifted,project(sc,lifted,'front',[b.x,5+b.h/2,b.z]),b.color));check('revision_is_live','D',Number(await page.getByTestId('revision').innerText())===(await(await fetch(base+'/api/state')).json()).revision);
// Exercise both load controls after the independently initiated lift.
for(let [key,label] of [['x','X'],['z','Z'],['y','Height'],['yaw','Yaw']])await page.getByRole('spinbutton',{name:label,exact:true}).fill(String({x:b.x,z:b.z,y:0,yaw:0}[key]));
await page.getByRole('button',{name:'Move',exact:true}).click();await page.waitForTimeout(350);
await page.getByRole('button',{name:'Release',exact:true}).click();await page.waitForTimeout(350);
check('ui_release','D',(await(await fetch(base+'/api/state')).json()).held===null);
await page.getByTestId('box-'+b.id).click();
await page.getByRole('spinbutton',{name:'Height',exact:true}).fill(String(b.h));
await page.getByRole('button',{name:'Move',exact:true}).click();await page.waitForTimeout(350);
await page.getByRole('button',{name:'Grip',exact:true}).click();await page.waitForTimeout(350);
check('ui_grip','D',(await(await fetch(base+'/api/state')).json()).held===b.id);
});
await scenario('orbit and zoom',async()=>{
await shot('iso');const orbitBefore=await canvas.screenshot();let area=await canvas.boundingBox();
await page.mouse.move(area.x+area.width/2,area.y+area.height/2);await page.mouse.down();await page.mouse.move(area.x+area.width/2+95,area.y+area.height/2+20,{steps:8});await page.mouse.up();
const orbitAfter=await canvas.screenshot();check('orbit_changes_view','D',!orbitBefore.equals(orbitAfter));
await page.mouse.wheel(0,-220);await page.waitForTimeout(200);check('zoom_changes_view','D',!orbitAfter.equals(await canvas.screenshot()));
});
await scenario('read-only camera',async()=>{
const beforeCamera=(await(await fetch(base+'/api/state')).json()).revision;await shot('top');check('camera_is_read_only','D',(await(await fetch(base+'/api/state')).json()).revision===beforeCamera);
});
check('clean_console','E',errors.length===0,errors.join('; '));
}catch(e){errors.push(String(e));check('browser_flow_failure','D',0,String(e));}
finally{await browser.close()}
// Missing checks stay zero rather than shrinking the denominator.
const expected={C:['webgl_geometry','seeded_box_geometry','columns_rails','bridge_beams','trolley_spreader','four_cables','spreader','wheels','crane_tracks_state','cargo_tracks_external_backend'],D:['scene_canvas_size','real_3d_pick','table_selection','ui_move_reaches_backend','visible_command_error','invalid_ui_move_is_atomic','revision_is_live','ui_release','ui_grip','orbit_changes_view','zoom_changes_view','camera_is_read_only'],E:['clean_console']};
for(let [tier,names] of Object.entries(expected))for(let name of names)if(!rows.some(r=>r.name===name))check(name,tier,0,'not reached: '+errors.join('; '));
if(process.argv[2]==='load'){console.log(JSON.stringify({renderedRowCount:0,consoleErrors:{count:errors.length,texts:errors}}));process.exit(0)}
console.log(JSON.stringify({checks:rows.filter(r=>r.name!=='browser_flow_failure'),errors}));
