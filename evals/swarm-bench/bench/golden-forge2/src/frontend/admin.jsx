import React, { useEffect, useState } from 'react';
import ForgeReconciler, { Button, DynamicTable, Heading, Inline, Label, SectionMessage, Stack, Text, Textfield, Toggle } from '@forge/react';
import { invoke } from '@forge/bridge';

// The Scope Ledger admin page (jira:adminPage, UI Kit). One invoke loads everything; every action is
// authorized again by its resolver (Jira ADMINISTER, asked as the caller), so this page only shows what
// the resolver answers.

const HEAD = {
  cells: [
    { key: 'when', content: 'When' },
    { key: 'who', content: 'Who' },
    { key: 'what', content: 'What' },
  ],
};

const formFrom = (settings) => ({
  backgroundShare: String(settings.backgroundShare),
  aiEnabled: settings.aiEnabled,
  dailyTokenBudget: String(settings.dailyTokenBudget),
  commentGroup: settings.commentGroup,
});

function Admin() {
  // One state object: the resolver's answer and the form edited from it render together.
  const [page, setPage] = useState(null);
  const [message, setMessage] = useState(null);
  const [newSecret, setNewSecret] = useState(null);
  const [busy, setBusy] = useState(false);

  const take = (res) => {
    if (res?.ok) setPage({ state: res, form: formFrom(res.settings) });
    else if (res?.forbidden) setPage({ state: res, form: null });
    return res;
  };

  useEffect(() => {
    invoke('getAdmin')
      .then((res) => {
        take(res);
        if (!res?.ok && !res?.forbidden) setMessage({ appearance: 'error', text: res?.error ?? 'The settings could not be loaded.' });
      })
      .catch((e) => setMessage({ appearance: 'error', text: `The settings could not be loaded: ${e?.message ?? e}` }));
  }, []);

  const run = async (key, payload, success) => {
    if (busy) return;
    setBusy(true);
    try {
      const res = take(await invoke(key, payload));
      if (res?.ok) {
        setMessage({ appearance: 'success', text: success(res) });
        if (res.newSecret) setNewSecret(res.newSecret);
      } else setMessage({ appearance: 'error', text: res?.error ?? 'The change was not saved.' });
    } catch (e) {
      setMessage({ appearance: 'error', text: `The change was not saved: ${e?.message ?? e}` });
    } finally {
      setBusy(false);
    }
  };

  const save = () => run('saveSettings', { settings: page.form }, (res) => (res.saved ? 'Settings saved.' : 'Nothing changed.'));
  const rotate = () => {
    setNewSecret(null);
    return run('rotateSecret', {}, () => 'A new CI secret was created. Copy it now: it is shown only once.');
  };
  const edit = (key, value) => setPage((p) => ({ ...p, form: { ...p.form, [key]: value } }));
  const set = (key) => (e) => edit(key, e?.target?.value ?? '');

  if (!page) {
    return (
      <Stack space="space.200">
        <Heading as="h2">Scope Ledger</Heading>
        {message ? <SectionMessage appearance={message.appearance}><Text>{message.text}</Text></SectionMessage> : <Text>Loading the settings…</Text>}
      </Stack>
    );
  }
  const { state, form } = page;
  if (state.forbidden) {
    return (
      <Stack space="space.200">
        <Heading as="h2">Scope Ledger</Heading>
        <SectionMessage appearance="warning">
          <Text>{state.error}</Text>
        </SectionMessage>
      </Stack>
    );
  }

  const migration = state.migration;
  return (
    <Stack space="space.300">
      <Heading as="h2">Scope Ledger</Heading>
      {message && (
        <SectionMessage appearance={message.appearance}>
          <Text>{message.text}</Text>
        </SectionMessage>
      )}

      <Stack space="space.100">
        <Label labelFor="background-share">Background share (%)</Label>
        <Textfield id="background-share" name="backgroundShare" type="number" min={10} max={90} value={form.backgroundShare} onChange={set('backgroundShare')} />
        <Label labelFor="ai-enabled">AI explanations enabled</Label>
        <Toggle id="ai-enabled" name="aiEnabled" label="AI explanations enabled" isChecked={form.aiEnabled} onChange={(e) => edit('aiEnabled', e?.target?.checked ?? !form.aiEnabled)} />
        <Label labelFor="token-budget">Daily AI token budget</Label>
        <Textfield id="token-budget" name="dailyTokenBudget" type="number" min={0} value={form.dailyTokenBudget} onChange={set('dailyTokenBudget')} />
        <Label labelFor="comment-group">Comment group</Label>
        <Textfield id="comment-group" name="commentGroup" placeholder="Empty: everyone who can browse the issue" value={form.commentGroup} onChange={set('commentGroup')} />
        <Inline space="space.100">
          <Button appearance="primary" isDisabled={busy} onClick={save}>
            Save settings
          </Button>
        </Inline>
      </Stack>

      <Stack space="space.100">
        <Heading as="h3">CI deployments</Heading>
        {newSecret ? (
          <Text>New CI secret (shown once): {newSecret}</Text>
        ) : (
          <Text>{state.secret ? `CI secret: ${state.secret.masked}` : 'No CI secret yet: rotate to create one.'}</Text>
        )}
        <Inline space="space.100">
          <Button isDisabled={busy} onClick={rotate}>
            Rotate CI secret
          </Button>
        </Inline>
      </Stack>

      <Stack space="space.100">
        <Label labelFor="migration-status">Migration</Label>
        <Textfield id="migration-status" name="migration" isReadOnly value={migration.text} />
        <Text>{migration.text}</Text>
      </Stack>

      <Stack space="space.100">
        <Heading as="h3">Recent admin changes</Heading>
        <DynamicTable
          caption="Recent admin changes"
          head={HEAD}
          rows={state.audit.map((e, i) => ({
            key: `${i}-${e.at}`,
            cells: [
              { key: 'when', content: e.at },
              { key: 'who', content: e.who },
              { key: 'what', content: e.what },
            ],
          }))}
          emptyView={<Text>No admin changes yet.</Text>}
        />
      </Stack>
    </Stack>
  );
}

ForgeReconciler.render(<Admin />);
