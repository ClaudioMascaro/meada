import { useId, useState } from 'react';
import type { Operation } from './actions';
import { recoverAction } from './actions';
import type { ActionsCopy } from './actions-i18n';

export interface ActionButtonsProps { onOpen: (operation: Operation) => void; busy: boolean; t: ActionsCopy }

export function ActionControls({ operations, onOpen, busy, t, identity }: ActionButtonsProps & { operations: Operation[]; identity: string }) {
  return <div className="action-list">{operations.map(operation => <div key={operation.action}>
    <button type="button" className="secondary" disabled={busy} aria-label={`${t[operation.action]} ${identity}`}
      onClick={() => onOpen(operation)}>{t[operation.action]}</button>
    <code>{operation.command}</code>
    {operation.action === 'set' && <p className="notice">{t.rollbackLimit}</p>}
  </div>)}</div>;
}

export function RecoveryControl({ file, identity, onOpen, busy, t }: ActionButtonsProps & { file: string; identity: string }) {
  const [floor, setFloor] = useState('');
  const [invalid, setInvalid] = useState(false);
  const id = useId();
  let command: string | undefined;
  try { command = recoverAction(floor, file).command; } catch { /* The form reports invalid floors on submit. */ }
  return <form className="fence-recovery" aria-label={`${t.recover} ${identity}`} onSubmit={event => {
    event.preventDefault();
    try { const operation = recoverAction(floor, file); setInvalid(false); onOpen(operation); }
    catch { setInvalid(true); }
  }}>
    <label htmlFor={id}>{t.floor}</label>
    <p id={`${id}-hint`} className="notice">{t.floorHint}</p>
    <input id={id} type="text" inputMode="numeric" value={floor} disabled={busy} aria-describedby={`${id}-hint`}
      aria-invalid={invalid} onChange={event => { setFloor(event.target.value); setInvalid(false); }} />
    <button type="submit" className="secondary" disabled={busy}>{t.recover}</button>
    {command && <code>{command}</code>}
    {invalid && <p className="error" role="alert">{t.floorInvalid}</p>}
  </form>;
}
