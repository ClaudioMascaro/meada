import { act, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { fireEvent, getByRole, queryByRole } from '@testing-library/dom';
import { afterEach, beforeAll, expect, test, vi } from 'vitest';
import { ConfirmDialog } from '../src/ConfirmDialog';

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const host = document.createElement('div');
document.body.append(host);
const root = createRoot(host);
beforeAll(() => {
  HTMLDialogElement.prototype.showModal = function () { this.open = true; };
  HTMLDialogElement.prototype.close = function () { this.open = false; };
});
afterEach(async () => { await act(() => root.render(null)); });

test('a labelled confirmation can be cancelled with Escape without accepting the operation', async () => {
  const accept = vi.fn();
  function Action() {
    const [open, setOpen] = useState(false);
    return <>
      <button onClick={() => setOpen(true)}>Set generation</button>
      {open && <ConfirmDialog title="Set generation" command="meada snapshot pointer set abc --workspace ops.toml"
        confirmLabel="Confirm set" cancelLabel="Cancel" busy={false}
        onConfirm={accept} onCancel={() => setOpen(false)} />}
    </>;
  }
  await act(() => root.render(<Action />));
  const opener = getByRole(host, 'button', { name: 'Set generation' });
  opener.focus();
  await act(() => fireEvent.click(opener));
  const dialog = getByRole(host, 'dialog', { name: 'Set generation' });
  expect(dialog.textContent).toContain('meada snapshot pointer set abc --workspace ops.toml');
  expect(getByRole(dialog, 'button', { name: 'Cancel' }).tabIndex).toBe(0);
  expect(getByRole(dialog, 'button', { name: 'Confirm set' }).tabIndex).toBe(0);
  expect(accept).not.toHaveBeenCalled();
  await act(() => fireEvent(dialog, new Event('cancel', { cancelable: true })));
  expect(queryByRole(host, 'dialog')).toBeNull();
  expect(document.activeElement).toBe(opener);
  expect(accept).not.toHaveBeenCalled();
});

test('an accepted operation stays visible while busy, then shows the server refusal', async () => {
  const accept = vi.fn();
  const cancel = vi.fn();
  const props = { title: 'Retain generation', command: 'meada snapshot pointer retain abc --workspace ops.toml',
    confirmLabel: 'Confirm retain', cancelLabel: 'Cancel', onConfirm: accept, onCancel: cancel };
  await act(() => root.render(<ConfirmDialog {...props} busy={false} />));
  await act(() => fireEvent.click(getByRole(host, 'button', { name: 'Confirm retain' })));
  expect(accept).toHaveBeenCalledOnce();
  await act(() => root.render(<ConfirmDialog {...props} busy />));
  const dialog = getByRole(host, 'dialog', { name: 'Retain generation' });
  const escape = new Event('cancel', { cancelable: true });
  await act(() => fireEvent(dialog, escape));
  expect(escape.defaultPrevented).toBe(true);
  expect(cancel).not.toHaveBeenCalled();
  expect(getByRole(dialog, 'button', { name: 'Cancel' }).hasAttribute('disabled')).toBe(true);
  expect(getByRole(dialog, 'button', { name: 'Confirm retain' }).hasAttribute('disabled')).toBe(true);
  await act(() => root.render(<ConfirmDialog {...props} busy={false} error="lock held by indexer / PID 42" />));
  expect(getByRole(host, 'alert').textContent).toBe('lock held by indexer / PID 42');
  await act(() => fireEvent(dialog, new Event('cancel', { cancelable: true })));
  expect(cancel).toHaveBeenCalledOnce();
});
