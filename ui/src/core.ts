import initialize, { init } from '../wasm/meada_core';
import wasmUrl from '../wasm/meada_core_bg.wasm?url';
import type { Language } from './i18n';

export { init, validate, workspaceVersionId, workspaceDiff, entriesText, entriesJson, authorizeEntries, composeEntries, parseExport, namedEntriesJson, namedVersionId, namedEntriesText, namedAuthorize, namedVisible, namedEffective, namedDiff, namedChanged } from '../wasm/meada_core';

export async function loadCore(language: Language): Promise<void> {
  await initialize({ module_or_path: wasmUrl });
  init(language);
}
