import { render, cleanup } from '@testing-library/react';
import { afterEach, expect, it } from 'vitest';
import { ScoreAdmission } from './ScoreAdmission';

afterEach(cleanup);
const admission = {
  visible: true,
  matching: true,
  good: true,
  excellence: { visual: true, backend: false },
  ceiling: 0.899,
  reasons: ['Recovery lost an acknowledged payment'],
};
it('shows independent excellence failures and the recorded ceiling without granting a floor', () => {
  const view = render(<ScoreAdmission admission={admission} rawScore={0.4} score={0.4} />);
  expect(view.getAllByText('0.400')).toHaveLength(2);
  view.getByText('0.899');
  view.getByText('Visual excellence: passed');
  view.getByText('Backend excellence: not met');
  view.getByText('Recovery lost an acknowledged payment');
});
it('states missing evidence rather than reconstructing gates', () => {
  const view = render(<ScoreAdmission score={0.8} />);
  view.getByText(/missing its score admission evidence/);
});
