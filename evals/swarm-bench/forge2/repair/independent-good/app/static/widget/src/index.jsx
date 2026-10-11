import { events,realtime,view } from '@forge/bridge';
import { boot,call } from '../../shared/bridge';
import { escaped,metrics } from '../../shared/dom';
import '../../shared/base.css';
import './widget.css';
const root=document.getElementById('root');let context,sub;const series=['committed','added','removed'];
function chart(sprints){const maximum=Math.max(0,...sprints.flatMap(s=>series.map(k=>s.values[k]))),width=Math.max(1,sprints.length*86+24);return `<figure class="chart"><svg data-testid="chart" class="chart-svg" viewBox="0 0 ${width} 160" preserveAspectRatio="none" role="img" aria-label="Sprint scope points">${sprints.flatMap((s,i)=>series.map((k,j)=>{const h=maximum?s.values[k]/maximum*160:0;return `<rect data-sprint-id="${escaped(s.id)}" data-series="${k}" class="series-${k}" x="${24+i*86+j*22}" y="${160-h}" width="18" height="${h}"><title>${escaped(s.name)}: ${escaped(s.text[k])}</title></rect>`;})).join('')}</svg><figcaption class="legend">${series.map(k=>`<span><span class="swatch series-${k}"></span>${k}</span>`).join('')}</figcaption></figure>`;}
async function load(){
 const boardId=context?.extension?.config?.boardId;
 if(boardId==null||boardId===''){root.innerHTML='<div data-testid="scope-widget" class="widget"><p data-testid="needs-config" class="message">Choose a scrum board in this widget’s settings.</p></div>';return;}
 try{const data=await call('widget',{boardId:String(boardId)});if(data.error)throw new Error(data.error);
 root.innerHTML=`<div data-testid="scope-widget" class="widget"><div class="sprints">${data.sprints.map(s=>`<section class="sprint" data-testid="sprint" data-sprint-id="${escaped(s.id)}"><h3 class="sprint-name">${escaped(s.name)}</h3>${metrics(s.text)}</section>`).join('')}</div>${chart(data.sprints)}</div>`;
 if(!sub&&data.realtime){sub={pending:true};sub=await realtime.subscribeGlobal(data.realtime.channel,()=>load());}
 }catch(e){root.innerHTML=`<div data-testid="scope-widget"><p class="message error" role="alert">${escaped(e.message)}</p></div>`;}
}
boot(async c=>{context=c;await load();await events.on('FORGE_DASHBOARDS_WIDGET_CONFIG_CHANGED',async()=>{context=await view.getContext();await load();});});
