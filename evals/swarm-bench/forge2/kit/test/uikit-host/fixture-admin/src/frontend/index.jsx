import React, { useEffect, useState } from 'react';
import ForgeReconciler, { Button, DynamicTable, ErrorMessage, Form, FormFooter, FormSection, Label, LoadingButton, Stack, Text, Textfield, Toggle, useForm } from '@forge/react';
import { invoke, requestJira, view } from '@forge/bridge';

const Preferences = ({ initial, onSaved }) => {
  const { register, handleSubmit, getFieldId, formState } = useForm({ defaultValues: initial });
  const [saving, setSaving] = useState(false);
  const save = async (values) => {
    setSaving(true);
    try {
      onSaved(await invoke('savePreferences', values));
    } finally {
      setSaving(false);
    }
  };
  return (
    <Form onSubmit={handleSubmit(save)}>
      <FormSection title="Preferences">
        <Label labelFor={getFieldId('retentionDays')}>Retention (days)</Label>
        <Textfield type="number" {...register('retentionDays', { required: true, min: 1, max: 365 })} />
        {formState.errors.retentionDays && <ErrorMessage>Enter a number of days from 1 to 365</ErrorMessage>}
        <Toggle label="Email digest enabled" {...register('digestEnabled')} />
      </FormSection>
      <FormFooter>
        <LoadingButton type="submit" appearance="primary" isLoading={saving}>Save preferences</LoadingButton>
      </FormFooter>
    </Form>
  );
};

const App = () => {
  const [state, setState] = useState(null);
  const [error, setError] = useState(null);
  const [flash, setFlash] = useState(null);
  const [me, setMe] = useState(null);
  const load = () => invoke('getPreferences').then(setState, (e) => setError(e.message));
  const whoAmI = async () => {
    const r = await requestJira('/rest/api/3/myself');
    setMe(`Signed in as ${(await r.json()).displayName} (${r.status})`);
  };
  useEffect(() => { load(); }, []);
  const saved = (p) => {
    setFlash(`Saved: retention ${p.retentionDays}, digest ${p.digestEnabled ? 'on' : 'off'}`);
    setTimeout(() => setFlash(null), 3000);
    load();
  };
  if (error) return <Text>Could not load preferences: {error}</Text>;
  if (!state) return <Text>Loading preferences…</Text>;
  return (
    <Stack space="space.200">
      <Preferences initial={{ retentionDays: state.retentionDays, digestEnabled: state.digestEnabled }} onSaved={saved} />
      {flash && <Text>{flash}</Text>}
      <DynamicTable
        caption="Change history"
        defaultSortKey="when"
        defaultSortOrder="DESC"
        head={{ cells: [{ key: 'when', content: 'When' }, { key: 'who', content: 'Who' }, { key: 'what', content: 'What' }] }}
        rows={state.changes.map((c) => ({ key: `change-${c.when}`, cells: [{ key: c.when, content: String(c.when) }, { key: c.who, content: c.who }, { key: c.what, content: c.what }] }))}
      />
      <Button onClick={() => view.theme.enable()}>Enable theming</Button>
      <Button onClick={whoAmI}>Check identity</Button>
      {me && <Text>{me}</Text>}
    </Stack>
  );
};

ForgeReconciler.render(<App />);
