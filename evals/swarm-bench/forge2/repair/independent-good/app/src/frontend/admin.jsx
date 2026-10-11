import React,{useEffect,useState} from 'react';
import ForgeReconciler,{Button,DynamicTable,Heading,Label,Stack,Text,Textfield,Toggle,SectionMessage} from '@forge/react';
import { invoke } from '@forge/bridge';
function Admin(){
 const [data,setData]=useState(null),[form,setForm]=useState(null),[error,setError]=useState(null),[secret,setSecret]=useState(null);
 const receive=res=>{if(res.error){setError(res.error);return;}const shown=res.secret;const masked=shown?.startsWith('CI secret: ')&&!shown.includes('••••')&&shown!=='CI secret: not configured'?'CI secret: ••••'+shown.slice(-4):shown;setData({...res,secret:masked});setForm(res.settings);setSecret(shown);};
 useEffect(()=>{invoke('adminLoad').then(receive).catch(e=>setError(e.message));},[]);
 const run=async(key,payload)=>{setError(null);setSecret(data?.secret);try{receive(await invoke(key,payload));}catch(e){setError(e.message);}};
 const field=(key,id,label)=> <React.Fragment key={key}><Label labelFor={id}>{label}</Label><Textfield id={id} type={key==='commentGroup'?'text':'number'} value={String(form[key])} onChange={e=>{setSecret(data.secret);setForm({...form,[key]:e.target.value});}} /></React.Fragment>;
 if(!form)return <Stack><Heading>Scope Ledger administration</Heading><Text>{error??'Loading settings…'}</Text></Stack>;
 const rows=data.audit.map((r,i)=>({key:String(i),cells:[{content:r.when},{content:r.who},{content:r.what}]}));
 return <Stack space="space.200"><Heading>Scope Ledger administration</Heading>{error&&<SectionMessage appearance="error"><Text>{error}</Text></SectionMessage>}
 {field('share','background-share','Background share (%)')}
 <Label labelFor="ai-enabled">AI explanations enabled</Label><Toggle id="ai-enabled" isChecked={form.aiEnabled} onChange={e=>{setSecret(data.secret);setForm({...form,aiEnabled:e.target.checked});}} />
 {field('tokenBudget','token-budget','Daily AI token budget')}{field('commentGroup','comment-group','Comment group')}
 <Button onClick={()=>run('adminSave',form)}>Save settings</Button><Button onClick={()=>run('adminRotate')}>Rotate CI secret</Button>
 <Text>{secret}</Text><Heading size="small">Migration</Heading><Text>{data.migration}</Text>
 <Heading size="small">Recent admin changes</Heading><DynamicTable head={{cells:[{key:'when',content:'when'},{key:'who',content:'who'},{key:'what',content:'what'}]}} rows={rows} />
 </Stack>;
}
ForgeReconciler.render(<Admin/>);
