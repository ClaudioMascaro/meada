# Roadmap

S1, S2 e S4 estão entregues. S3 continua planejado e S5 começou: o catálogo elegível, o primeiro workspace nomeado, o ciclo de vida de suas versões, a configuração por workspace, a autoria guiada e as propostas MCP estão entregues. Este mapa mostra dependências e limites de escopo, sem prometer datas.

<!-- meada-graph:roadmap -->

## O que já está disponível

[S1, #259](https://github.com/ClaudioMascaro/meada/issues/259) entrega as bases do [modo servidor](server.md). [S2, #293](https://github.com/ClaudioMascaro/meada/issues/293) entrega o [console](console.md), versões imutáveis e operações da frota.

## Bases em construção

[S3, #294](https://github.com/ClaudioMascaro/meada/issues/294) trata de owners e autorização das entradas. [S4, #319](https://github.com/ClaudioMascaro/meada/issues/319), já entregue com a [identidade empresarial](enterprise.md), trata de identidade, grants, diretório e auditoria. Partes dessas bases podem avançar em paralelo; o grafo dos tickets determina os bloqueios reais. O mapa não representa S3 inteiro como pré-requisito de S4 inteiro.

S5 depende das duas bases, da [publicação atômica, #316](https://github.com/ClaudioMascaro/meada/issues/316), e da [medição e validação, #338](https://github.com/ClaudioMascaro/meada/issues/338).

## Depois das bases

[S5, #339](https://github.com/ClaudioMascaro/meada/issues/339) introduzirá [workspaces nomeados](named-workspaces.md) sobre indexação compartilhada. A [consolidação de contratos, #340](https://github.com/ClaudioMascaro/meada/issues/340), está concluída (ADRs 0020 a 0022) e vem antes do runtime. A sequência aceita é contratos → catálogo elegível → primeiro workspace → versões e sobreposição → configuração entre repositórios. O catálogo elegível ([#341](https://github.com/ClaudioMascaro/meada/issues/341)) está entregue: descobrir os repositórios selecionáveis antes de existir um manifesto, sem criar workspace nem índice. O primeiro workspace ([#342](https://github.com/ClaudioMascaro/meada/issues/342)) também está entregue: criação a partir do catálogo, ingestão compartilhada com a regra do patrocinador e a rota de nó `/-/workspaces/<id>/<identity>`; o ciclo de vida das versões ([#343](https://github.com/ClaudioMascaro/meada/issues/343)) também está entregue: salvar, aplicar, reverter, renomear, apagar e delegar, com uma base que mantém separados os workspaces sobrepostos e os editores simultâneos; a configuração por workspace ([#344](https://github.com/ClaudioMascaro/meada/issues/344)) também está entregue, e a autoria guiada ([#345](https://github.com/ClaudioMascaro/meada/issues/345)) também: controles de formulário para repositórios, peers, configurações de serve, recursos e nomes de variáveis de ambiente que continuam sendo um só documento com o texto do manifesto. A medição contra uma organização real do GitHub ([#338](https://github.com/ClaudioMascaro/meada/issues/338)) continua pendente e é uma condição para a aceitação empresarial, não para estas etapas. As propostas MCP autenticadas ([#346](https://github.com/ClaudioMascaro/meada/issues/346)) também estão entregues. A validação empresarial vem a seguir.

As especificações vinculadas são a referência para o contrato completo. Um item planejado neste roadmap não é uma opção de configuração disponível.
