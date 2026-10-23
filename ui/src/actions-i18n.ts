import type { Language } from './i18n';

const en = {
  actions: 'Actions', set: 'Set pointer', retain: 'Retain generation', drop: 'Drop retention', recover: 'Recover fence',
  confirm: 'Confirm', cancel: 'Cancel',
  rollbackLimit: 'This pointer holds until the next publish. Suspend the refresh job to stay on this generation.',
  floor: 'Minimum fence (optional)', floorHint: 'An unsigned integer, up to 18446744073709551615.',
  floorInvalid: 'Enter a whole number from 0 to 18446744073709551615, or leave the floor empty.',
  manifestError: 'Actions are unavailable because the bootstrap manifest could not be loaded.',
  reloadError: 'The operation succeeded, but the fleet could not be reloaded.',
};
export type ActionsCopy = { [K in keyof typeof en]: string };
export const actionsCopy: Record<Language, ActionsCopy> = {
  en,
  'pt-BR': {
    actions: 'Ações', set: 'Mudar pointer', retain: 'Manter geração', drop: 'Remover retenção', recover: 'Recuperar fence',
    confirm: 'Confirmar', cancel: 'Cancelar',
    rollbackLimit: 'Este pointer vale até o próximo publish. Suspenda o job de refresh para permanecer nesta geração.',
    floor: 'Fence mínimo (opcional)', floorHint: 'Um inteiro sem sinal, até 18446744073709551615.',
    floorInvalid: 'Informe um inteiro de 0 a 18446744073709551615, ou deixe o piso vazio.',
    manifestError: 'As ações estão indisponíveis porque não foi possível carregar o manifesto de bootstrap.',
    reloadError: 'A operação foi concluída, mas não foi possível atualizar a frota.',
  },
};
