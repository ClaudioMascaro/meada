# Alertas e encaminhamento de incidentes

Leitura estática de regras e rotas de monitoramento, configuração de incidentes e encaminhamentos de receptores do Alertmanager.

## Interface

O leitor resume as regras, rotas e receptores dos arquivos de configuração. A partir de uma rota e uma regra, o encaminhamento produz eventos para receptores conhecidos. `[config.rs:read]` `[incident.rs:terraform]` `[forwarding.rs:events]`

## Invariantes

Expressões regulares respeitam o subconjunto Go/RE2 aceito e os limites de custo. Uma condição PCL fora do subconjunto literal permanece desconhecida. `[regexp.rs:compile]` `[pcl.rs:parse]`

Uma condição PCL com mais de 64 níveis de grupos ou operadores unários fica sem avaliação. `[pcl.rs:ALERT_PCL_DEPTH_MAX]`

## Efeitos colaterais

A análise gera resumos e eventos em memória. O encaminhamento substitui os modelos admitidos sem executá-los ou enviar notificações. `[config.rs:Summary]` `[forwarding.rs:events]`

## Verificação

Na raiz do repositório, execute `cargo test -p meada --test it alerts::`.
