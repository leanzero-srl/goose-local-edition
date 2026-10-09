import React, { useEffect, useState } from 'react';
import ForgeReconciler, { Button, DynamicTable, ErrorMessage, Form, FormFooter, FormSection, Label, LoadingButton, Stack, Text, Textfield, Toggle, useForm } from '@forge/react';
import { invoke, requestJira, view } from '@forge/bridge';

const Settings = ({ initial, onSaved }) => {
  const { register, handleSubmit, getFieldId, formState } = useForm({ defaultValues: initial });
  const [saving, setSaving] = useState(false);
  const save = async (values) => {
    setSaving(true);
    try {
      onSaved(await invoke('saveSettings', values));
    } finally {
      setSaving(false);
    }
  };
  return (
    <Form onSubmit={handleSubmit(save)}>
      <FormSection title="Settings">
        <Label labelFor={getFieldId('backgroundShare')}>Background share (%)</Label>
        <Textfield type="number" {...register('backgroundShare', { required: true, min: 10, max: 90 })} />
        {formState.errors.backgroundShare && <ErrorMessage>Enter a number from 10 to 90</ErrorMessage>}
        <Toggle label="AI explanations enabled" {...register('aiEnabled')} />
      </FormSection>
      <FormFooter>
        <LoadingButton type="submit" appearance="primary" isLoading={saving}>Save settings</LoadingButton>
      </FormFooter>
    </Form>
  );
};

const App = () => {
  const [state, setState] = useState(null);
  const [error, setError] = useState(null);
  const [flash, setFlash] = useState(null);
  const [me, setMe] = useState(null);
  const load = () => invoke('getSettings').then(setState, (e) => setError(e.message));
  const whoAmI = async () => {
    const r = await requestJira('/rest/api/3/myself');
    setMe(`Signed in as ${(await r.json()).displayName} (${r.status})`);
  };
  useEffect(() => { load(); }, []);
  const saved = (s) => {
    setFlash(`Saved: share ${s.backgroundShare}, AI ${s.aiEnabled ? 'on' : 'off'}`);
    setTimeout(() => setFlash(null), 3000);
    load();
  };
  if (error) return <Text>Could not load settings: {error}</Text>;
  if (!state) return <Text>Loading settings…</Text>;
  return (
    <Stack space="space.200">
      <Settings initial={{ backgroundShare: state.backgroundShare, aiEnabled: state.aiEnabled }} onSaved={saved} />
      {flash && <Text>{flash}</Text>}
      <DynamicTable
        caption="Recent admin changes"
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
