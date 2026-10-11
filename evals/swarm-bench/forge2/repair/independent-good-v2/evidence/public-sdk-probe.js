import api, { route } from '@forge/api';
export async function inspect(payload) {
  const result = {boards:[],sprints:[],issues:[]};
  for(const id of payload.boards??[]) {
    const response=await api.asApp().requestJira(route`/rest/agile/1.0/board/${id}/configuration`);
    if(!response.ok)throw new Error(`Board read: ${response.status}`);
    result.boards.push(await response.json());
  }
  for(const id of payload.sprints??[]) {
    const response=await api.asApp().requestJira(route`/rest/agile/1.0/sprint/${id}`);
    if(!response.ok)throw new Error(`Sprint read: ${response.status}`);
    result.sprints.push(await response.json());
  }
  const ids=payload.issues??[];
  for(let i=0;i<ids.length;i+=100) {
    const response=await api.asApp().requestJira(route`/rest/api/3/issue/bulkfetch`,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({issueIdsOrKeys:ids.slice(i,i+100),fields:payload.fields,expand:['changelog']})});
    if(!response.ok)throw new Error(`Issue read: ${response.status}`);
    result.issues.push(...(await response.json()).issues);
  }
  return result;
}
