# Roadmap

S1 e S2 estão entregues. S3, S4 e S5 continuam planejados. Este mapa mostra dependências e limites de escopo, sem prometer datas.

<!-- meada-graph:roadmap -->

## O que já está disponível

[S1, #259](https://github.com/ClaudioMascaro/meada/issues/259) entrega as bases do [modo servidor](server.md). [S2, #293](https://github.com/ClaudioMascaro/meada/issues/293) entrega o [console](console.md), versões imutáveis e operações da frota.

## Bases em construção

[S3, #294](https://github.com/ClaudioMascaro/meada/issues/294) trata de owners e autorização das entradas. [S4, #319](https://github.com/ClaudioMascaro/meada/issues/319) trata de identidade, grants, diretório e auditoria. Partes dessas bases podem avançar em paralelo; o grafo dos tickets determina os bloqueios reais. O mapa não representa S3 inteiro como pré-requisito de S4 inteiro.

S5 depende das duas bases, da [publicação atômica, #316](https://github.com/ClaudioMascaro/meada/issues/316), e da [medição e validação, #338](https://github.com/ClaudioMascaro/meada/issues/338).

## Depois das bases

[S5, #339](https://github.com/ClaudioMascaro/meada/issues/339) introduzirá [workspaces nomeados](named-workspaces.md) sobre indexação compartilhada. A [consolidação de contratos, #340](https://github.com/ClaudioMascaro/meada/issues/340), vem antes do runtime. A sequência aceita é contratos → catálogo elegível → primeiro workspace → versões e sobreposição → configuração entre repositórios. Depois, UI guiada e propostas MCP avançam em paralelo; a validação empresarial depende de ambas.

As especificações vinculadas são a referência para o contrato completo. Um item planejado neste roadmap não é uma opção de configuração disponível.
