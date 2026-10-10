import React, { useEffect, useState } from 'react';
import ForgeReconciler, { Button, DynamicTable, Heading, Label, SectionMessage, Stack, Text, Textfield, Toggle } from '@forge/react';
import { invoke } from '@forge/bridge';

const toForm = (settings) => ({
  backgroundShare: String(settings.backgroundShare),
  aiEnabled: settings.aiEnabled === true,
  tokenBudget: String(settings.tokenBudget),
  commentGroup: settings.commentGroup ?? '',
});

const HEAD = { cells: ['when', 'who', 'what'].map((key) => ({ key, content: key })) };

function App() {
  const [panel, setPanel] = useState(null);
  const [form, setForm] = useState(null);
  const [error, setError] = useState(null);
  const [notice, setNotice] = useState(null);
  const [busy, setBusy] = useState(false);

  const apply = (res, { keepForm = false } = {}) => {
    if (!res || typeof res !== 'object') return setError('The admin page got no answer.');
    if (res.settings) {
      setPanel(res);
      if (!keepForm) setForm(toForm(res.settings));
    }
    setError(res.error ?? null);
  };

  useEffect(() => {
    invoke('load')
      .then((res) => apply(res))
      .catch((e) => setError(e?.message ?? String(e)));
  }, []);

  // The revealed secret belongs to the rotation's answer only: any later interaction shows the masked form again.
  const hideSecret = () => {
    if (panel?.revealed) setPanel({ ...panel, revealed: false, secret: panel.maskedSecret });
  };

  const update = (key, value) => {
    hideSecret();
    setNotice(null);
    setForm((f) => ({ ...f, [key]: value }));
  };

  const run = async (key, payload) => {
    if (busy) return;
    setBusy(true);
    setNotice(null);
    hideSecret();
    try {
      const res = await invoke(key, payload);
      apply(res, { keepForm: Boolean(res?.error) });
      if (!res?.error && key === 'save') setNotice(res.changed ? 'Settings saved.' : 'Nothing changed.');
      if (!res?.error && key === 'rotate') setNotice('A new CI secret was made. Copy it now: it is shown only this once.');
    } catch (e) {
      setError(e?.message ?? String(e));
    } finally {
      setBusy(false);
    }
  };

  if (!panel || !form) {
    return (
      <Stack space="space.200">
        <Heading as="h2">Scope Ledger</Heading>
        {error ? (
          <SectionMessage appearance="error">
            <Text>{error}</Text>
          </SectionMessage>
        ) : (
          <Text>Loading the settings…</Text>
        )}
      </Stack>
    );
  }

  const rows = (panel.audit ?? []).map((r, i) => ({
    key: `audit-${i}`,
    cells: [
      { key: 'when', content: r.when },
      { key: 'who', content: r.who },
      { key: 'what', content: r.what },
    ],
  }));

  return (
    <Stack space="space.200">
      <Heading as="h2">Scope Ledger</Heading>
      {error && (
        <SectionMessage appearance="error">
          <Text>{error}</Text>
        </SectionMessage>
      )}
      {notice && (
        <SectionMessage appearance="success">
          <Text>{notice}</Text>
        </SectionMessage>
      )}
      <Label labelFor="background-share">Background share (%)</Label>
      <Textfield id="background-share" type="number" min={10} max={90} value={form.backgroundShare} onChange={(e) => update('backgroundShare', e.target.value)} />
      <Label labelFor="ai-enabled">AI explanations enabled</Label>
      <Toggle id="ai-enabled" isChecked={form.aiEnabled} onChange={() => update('aiEnabled', !form.aiEnabled)} />
      <Label labelFor="token-budget">Daily AI token budget</Label>
      <Textfield id="token-budget" type="number" min={0} value={form.tokenBudget} onChange={(e) => update('tokenBudget', e.target.value)} />
      <Label labelFor="comment-group">Comment group</Label>
      <Textfield id="comment-group" value={form.commentGroup} placeholder="Empty: anyone who can browse the issue" onChange={(e) => update('commentGroup', e.target.value)} />
      <Button appearance="primary" isDisabled={busy} onClick={() => run('save', form)}>
        Save settings
      </Button>
      <Text>{panel.secret}</Text>
      <Button isDisabled={busy} onClick={() => run('rotate')}>
        Rotate CI secret
      </Button>
      <Label labelFor="migration">Migration</Label>
      <Textfield id="migration" isReadOnly value={panel.migration} />
      <DynamicTable caption="Recent admin changes" head={HEAD} rows={rows} emptyView="No admin changes yet." />
    </Stack>
  );
}

ForgeReconciler.render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
