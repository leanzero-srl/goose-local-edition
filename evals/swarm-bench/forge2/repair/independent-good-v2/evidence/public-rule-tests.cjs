const fs = require('fs');
const assert = require('assert/strict');
(async () => {
  const source = fs.readFileSync('src/estimates.js','utf8');
  const {estimateValue, estimateAt} = await import('data:text/javascript;base64,'+Buffer.from(source).toString('base64'));
  assert.equal(estimateValue({points:null},'points'),0);
  assert.equal(estimateValue({points:8.25},'points'),8.25);
  const history = [
    {id:'2',created:'2026-01-01T02:00:00Z',items:[{fieldId:'points',from:'5',to:'8'}]},
    {id:'3',created:'2026-01-01T03:00:00Z',items:[{fieldId:'points',from:'8',to:'13'}]},
    {id:'1',created:'2026-01-01T01:00:00Z',items:[{fieldId:'points',from:null,to:'5'}]},
  ];
  assert.equal(estimateAt({points:13},'points',history,Date.parse('2026-01-01T02:30:00Z')),8);
  assert.equal(estimateAt({points:13},'points',history,Date.parse('2026-01-01T01:30:00Z')),5);
  assert.equal(estimateAt({points:13},'points',history,Date.parse('2026-01-01T00:30:00Z')),0);
  assert.equal(estimateAt({points:13},'points',history,Date.parse('2026-01-01T02:00:00Z')),8);
  assert.equal(estimateAt({points:13},'points',[{id:'2',created:'2026-01-01T03:00:00Z',items:[{fieldId:'points',from:'5',to:'8'}]},{id:'3',created:'2026-01-01T03:00:00Z',items:[{fieldId:'points',from:'8',to:'13'}]}],Date.parse('2026-01-01T02:30:00Z')),5);
  const jira = fs.readFileSync('src/jira.js','utf8');
  const cost = Function(jira.slice(jira.indexOf('function actualCost('),jira.indexOf('export async function jiraJson('))+';return actualCost;')();
  assert.equal(cost({id:'1',fields:{nested:[1,2,3]}},'/rest/api/3/issue/1',{}),2);
  assert.equal(cost({issues:[]},'/rest/api/3/search/jql',{method:'POST'}),1);
  assert.equal(cost({issues:[{},{}]},'/rest/api/3/search/jql',{method:'POST'}),3);
  assert.equal(cost([{},{}],'/rest/api/3/user/groups',{}),5);
  assert.equal(cost(undefined,'/rest/api/3/app/field/1/value',{method:'PUT'}),1);
  assert.equal(require('crypto').createHash('sha256').update(fs.readFileSync('src/explain.js')).digest('hex'),'042640427f7aea00f2d8ef8370f9aa92574f11690e6eb1f19637c38cb4ce6a74');
  console.log('PASS: estimate close reconstruction (unordered history, null, exact boundary); current-value normalization; single/list/identity/write costs; unchanged awaited LLM implementation.');
})();
