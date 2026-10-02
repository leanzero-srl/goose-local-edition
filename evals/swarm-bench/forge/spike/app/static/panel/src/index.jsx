import React, { useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { invoke, view, requestJira } from '@forge/bridge';

function App() {
  const [ctx, setCtx] = useState(null);
  const [data, setData] = useState(null);
  const [other, setOther] = useState(null);
  const [posted, setPosted] = useState(null);
  const [err, setErr] = useState(null);
  useEffect(() => {
    (async () => {
      try {
        const c = await view.getContext();
        setCtx(c);
        setData(await invoke('summarise', {}));
        const r = await requestJira('/rest/api/3/issue/SPK-2');
        setOther(await r.json());
      } catch (e) { setErr(String(e.message ?? e)); }
    })();
  }, []);
  const comment = async () => setPosted(await invoke('comment', { issueKey: ctx.extension.issue.key, text: 'clicked in the panel' }));
  if (err) return <div className="err">Error: {err}</div>;
  if (!data) return <div>Loading…</div>;
  return (
    <div className="card">
      <h2>{data.key}: {data.summary}</h2>
      <p>Status <b className="pill">{data.status}</b> · viewed {data.views}×</p>
      <p>Neighbour {other?.key}: {other?.fields?.summary}</p>
      <p className="muted">module {ctx.moduleKey} · site {ctx.siteUrl}</p>
      <button onClick={comment}>Add comment</button>
      {posted && <p id="posted">Comment status {posted.status}</p>}
    </div>
  );
}
view.theme?.enable?.();
createRoot(document.getElementById('root')).render(<App />);
