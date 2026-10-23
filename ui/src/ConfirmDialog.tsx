import { useEffect, useId, useRef } from 'react';
import './confirm.css';

interface Props {
  title: string; command: string; confirmLabel: string; cancelLabel: string;
  busy: boolean; error?: string; onConfirm: () => void; onCancel: () => void;
}

export function ConfirmDialog({ title, command, confirmLabel, cancelLabel, busy, error, onConfirm, onCancel }: Props) {
  const dialog = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  const commandId = useId();
  useEffect(() => {
    const opener = document.activeElement;
    const element = dialog.current;
    element?.showModal();
    return () => {
      element?.close();
      if (opener instanceof HTMLElement && opener.isConnected) opener.focus();
    };
  }, []);
  return <dialog ref={dialog} className="confirmation" aria-labelledby={titleId} aria-describedby={commandId} aria-busy={busy}
    onCancel={event => { event.preventDefault(); if (!busy) onCancel(); }}>
    <h2 id={titleId}>{title}</h2>
    <pre id={commandId}><code>{command}</code></pre>
    {error && <p className="error" role="alert">{error}</p>}
    <div className="confirmation-buttons">
      <button type="button" className="secondary" autoFocus disabled={busy} onClick={onCancel}>{cancelLabel}</button>
      <button type="button" disabled={busy} onClick={onConfirm}>{confirmLabel}</button>
    </div>
  </dialog>;
}
