import { expect, it } from 'vitest';
import { publicScoreDetails } from './benchPublicScore';
it('preserves recorded composition, conditions and critical rows without manufacturing missing inputs', () => {
  const verdict = {inner:.9263,excellence:{fraction:.9384,e_mean:.75,conditions:[{name:'j_workflow_journey',ok:false,value:.714285714}]},critical:{floor:.6,multiplier:.8857,pre_severity_score:.8996,rows:[{check:'j_workflow_journey',score:.714285714,severity_input:.7143,factor:.8857,why:'dead primary flow'}]}};
  expect(publicScoreDetails(verdict)).toEqual({scoreInner:.9263,excellenceFraction:.9384,excellenceEMean:.75,criticalFloor:.6,criticalMultiplier:.8857,preSeverityScore:.8996,gateConditions:verdict.excellence.conditions,criticalRows:[{check:'j_workflow_journey',score:.714285714,factor:.8857,why:'dead primary flow',severity_input:.7143}]});
  expect(publicScoreDetails({})).toEqual({});
});

it('retains root suppression without inventing an additional penalty', () => { expect(publicScoreDetails({critical:{rows:[{check:'sibling',score:0,factor:1,why:'same root',suppressed:'root:primary',severity_input:0}]}}).criticalRows).toEqual([{check:'sibling',score:0,factor:1,why:'same root',suppressed:'root:primary',severity_input:0}]); });
