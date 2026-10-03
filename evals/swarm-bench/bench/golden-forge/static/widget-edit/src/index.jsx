import React, { useEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { widgetEdit } from '@forge/dashboards-bridge';
import { boot, call } from '../../shared/bridge';
import '../../shared/base.css';
import './edit.css';

function Edit({ context }) {
  const stored = context?.extension?.config ?? {};
  const [boards, setBoards] = useState(null);
  const [error, setError] = useState(null);
  const [selected, setSelected] = useState(stored.boardId === undefined || stored.boardId === null ? null : String(stored.boardId));
  const selectedRef = useRef(selected);

  useEffect(() => {
    // The dashboard's own Save stores what this returns; with no board chosen there is nothing to store.
    widgetEdit
      .onProductSave(async (config) => (selectedRef.current ? { ...(config ?? {}), boardId: selectedRef.current } : null))
      .catch((e) => setError(`The dashboard did not accept the save handler: ${e.message}`));
    call('boards')
      .then((res) => (res.ok === false ? setError(res.error) : setBoards(res.boards)))
      .catch((e) => setError(e?.message ?? String(e)));
  }, []);

  const choose = (board) => {
    selectedRef.current = board.id;
    setSelected(board.id);
    widgetEdit.updateConfig({ ...stored, boardId: board.id }).catch((e) => setError(e.message));
  };

  return (
    <div data-testid="scope-widget-edit" className="edit">
      <h2 id="board-heading">Scrum board</h2>
      <p className="subtle">The widget shows the active sprints of the board you choose.</p>
      {error && (
        <p className="message error" role="alert">
          {error}
        </p>
      )}
      {!boards && !error && <p className="subtle">Loading boards…</p>}
      {boards && boards.length === 0 && <p className="message">This site has no scrum boards.</p>}
      {boards && boards.length > 0 && (
        <ul className="board-list" aria-labelledby="board-heading">
          {boards.map((b) => (
            <li key={b.id}>
              <button
                type="button"
                data-testid="board-option"
                data-board-id={b.id}
                aria-pressed={selected === b.id ? 'true' : 'false'}
                className={`board-option${selected === b.id ? ' selected' : ''}`}
                onClick={() => choose(b)}
              >
                <span className="board-name">{b.name}</span>
                <span className="board-id">#{b.id}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

boot((context) => createRoot(document.getElementById('root')).render(<Edit context={context} />));
