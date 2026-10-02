import React, { useEffect, useState } from 'react';
import ForgeReconciler, { Heading, Text, Button, Lozenge, Stack, useProductContext } from '@forge/react';
import { invoke } from '@forge/bridge';

const App = () => {
  const context = useProductContext();
  const [data, setData] = useState(null);
  const [posted, setPosted] = useState(null);
  useEffect(() => { invoke('summarise', {}).then(setData); }, []);
  if (!data) return <Text>Loading…</Text>;
  return (
    <Stack space="space.100">
      <Heading as="h2">{data.key}: {data.summary}</Heading>
      <Text>Status <Lozenge appearance="inprogress">{data.status}</Lozenge> · viewed {data.views}×</Text>
      <Text>module {context?.moduleKey}</Text>
      <Button appearance="primary" onClick={async () => setPosted(await invoke('comment', { issueKey: data.key, text: 'from UI Kit' }))}>Add comment</Button>
      {posted && <Text>Comment status {posted.status}</Text>}
    </Stack>
  );
};
ForgeReconciler.render(<React.StrictMode><App /></React.StrictMode>);
