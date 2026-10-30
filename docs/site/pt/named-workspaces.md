# Workspaces nomeados

!!! warning "Parcialmente entregue: S5, contratos decididos; o catálogo, o primeiro workspace e suas versões já existem"
    [S5, #339](https://github.com/ClaudioMascaro/meada/issues/339) é escopo sequencial aceito após S3 e S4. A [consolidação dos contratos, #340](https://github.com/ClaudioMascaro/meada/issues/340), está concluída: os ADRs 0020, 0021 e 0022 fixam os contratos descritos aqui. Do runtime, duas fatias estão entregues: o **catálogo** ([#341](https://github.com/ClaudioMascaro/meada/issues/341)), descrito em [o catálogo](github-app.md#o-catalogo-de-repositorios) e no [console](console.md#catalogo-de-repositorios), e o **primeiro workspace** ([#342](https://github.com/ClaudioMascaro/meada/issues/342)): `[workspaces]`, criação, listagem e consulta, ingestão compartilhada com a regra do patrocinador e a rota do nó, descritos no [console](console.md#workspaces-nomeados). O **ciclo de vida das versões** ([#343](https://github.com/ClaudioMascaro/meada/issues/343)) também está entregue: salvar, aplicar, reverter, renomear, apagar e delegar, com a leitura de versões e histórico, descrito no [console](console.md#workspaces-nomeados) e abaixo. A configuração por workspace, a autoria guiada e as propostas MCP não foram construídas, e o restante abaixo não é uma opção de configuração por enquanto. Sem `[workspaces]` o console continua operando com um workspace por deployment.

## Compor sem duplicar a indexação

Workspaces nomeados se sobrepõem sobre a indexação compartilhada. Um repositório não ganha outro índice, job, trava de refresh ou geração por pertencer a dois workspaces: um único refresh busca, indexa e publica cada repositório selecionado uma vez, e cada workspace é um contexto que seleciona alguns deles. Há três papéis humanos: administrador da plataforma, operador de workspace e viewer.

O grant efetivo por repositório continua sendo a autoridade. Com Y legível pelos grupos A e B, o repositório A só pelo grupo A e o repositório B só pelo grupo B, um operador apenas em A pode compor Y+A e tem Y+B recusado; um operador apenas em B pode compor Y+B e tem Y+A recusado; quem está nos dois pode compor os três. Compartilhar Y não concede A nem B a ninguém, e um link de Y para B não aparece no workspace de Y+A.

<!-- meada-graph:workspace-overlap -->

## O primeiro workspace

Um mantenedor admitido, sem nenhuma entrada própria, abre **Workspaces nomeados** no console, escolhe repositórios do catálogo, dá um nome ao workspace e confirma o manifesto que a página mostra. O servidor decide de novo em uma transação contra o conjunto de permissões mais novo: todo repositório selecionado precisa estar no catálogo e o login do GitHub da pessoa precisa ter um papel aceito nele, direto ou por um grupo. Uma recusa nomeia os repositórios que a concessão da pessoa lê e apenas conta os demais. O workspace é gravado na identidade da pessoa, que o opera; colegas cujas concessões próprias leem os mesmos repositórios o veem sem convite algum e não podem alterá-lo.

O workspace passa a poder ser consultado pelo job compartilhado: `meada workspace pull` o lê, `meada github-app roles` deixa de fora, antes da busca, toda entrada em que nenhum de seus operadores tem um papel aceito (uma decisão de política, não uma busca que falhou) e os repositórios que só workspaces nomeados selecionam são buscados, indexados e publicados uma vez, em `<[workspaces] mirrors>/<identity>`. Até uma geração registrar o workspace, o console o mostra como `applied` com as entradas `pending`; depois de registrado ele é `live` e as entradas `ready`. Uma busca ou indexação que falha continua abortando toda a publicação e mantendo a geração atual.

## Manter um workspace

Um operador edita seu workspace como uma sequência de versões. **Salvar** guarda uma versão e não move nada; **aplicar** torna uma versão guardada a atual, e aplicar uma que já foi a atual é um **rollback**. Toda mudança leva a **base** que a página leu (o número que `GET /api/v1/workspaces/<id>` mostra): se alguém mudou o workspace desde então, a mudança é recusada (`stale_base`, informando a base atual) e nada é gravado, de modo que duas pessoas editando ao mesmo tempo nunca se sobrescrevem. No momento da gravação o servidor confere de novo, em uma transação, que o workspace ainda existe, que a base é a atual, que a pessoa ainda o opera (ou é o administrador da plataforma) e que todo repositório que a mudança **adiciona ou altera** ainda é elegível para ela no conjunto de permissões mais novo; os repositórios que uma versão mantém sem alteração e as remoções não são reconferidos. Um papel revogado, uma entrada recusada, uma página velha ou uma versão de outro workspace não mudam nada.

Um operador que perdeu o papel em um repositório mantém o workspace, lê as entradas que sua concessão lê e uma contagem das demais, e ainda pode editá-las: a versão que salva mantém no lugar, na ordem, as entradas que ele não lê, e ele não consegue lê-las nem sobrescrevê-las. Ele remove uma voltando a uma versão que não a tem, pedindo ao administrador da plataforma (cuja visão de gestão não é filtrada) ou apagando o workspace. Os colegas que perderam o papel em qualquer um dos repositórios deixam de ver o workspace, suas versões e seu histórico: recebem o 404 de um id que não existe. Um visualizador, isto é, quem lê o workspace sem operá-lo, não escreve: recebe um 403 `not_operator`; e ler um repositório nunca edita o workspace de outra pessoa.

**Renomear** muda só o nome e a descrição (o id continua). **Apagar** tira o workspace das listas e do próximo refresh, mantendo suas versões e seu histórico no Postgres; os repositórios que ele selecionava continuam indexados uma única vez para cada outro workspace que os selecione, e as gerações retidas mantêm o contexto até serem descartadas. Só o administrador da plataforma **delega** operadores (um workspace mantém ao menos um). Workspaces sobrepostos sobre o mesmo repositório são independentes: editar, reverter ou apagar um não move nada no outro. Cada operação, recusas incluídas, é um registro de auditoria com a pessoa, o workspace e a base; `meada workspaces list|show|versions|diff|create|save|apply|rename|delete|operators` fazem o mesmo pela linha de comando, como administrador da plataforma.

## Quem pode o quê

- **Administrador da plataforma**: o operador global que já existe. Configura o deployment, delega operadores e administra qualquer workspace. Nenhum papel serializado muda de nome.
- **Operador de workspace**: quem criou um workspace ou recebeu a delegação dele. Edita, aplica, reverte, renomeia e remove esse workspace e nenhum outro. Estar no mesmo grupo do criador não concede nada disso.
- **Viewer**: ninguém o atribui. Uma pessoa vê um workspace quando o próprio grant atual lê todos os repositórios dele, então colegas de grupo não precisam de convite, de grupo no provedor de identidade nem de pacote de acesso.

Criar um workspace exige um papel aceito do GitHub, direto ou por grupo do repositório, em todos os repositórios selecionados. O grant por claims continua valendo para leitura, não para criação. A elegibilidade nunca é armazenada: cada decisão lê o conjunto de permissões mais recente, e um conjunto mais antigo que `max_age_seconds` torna nada selecionável até o job gravar outro.

## O que um leitor parcial vê

Um workspace fica oculto para quem tem grant que não lê todos os repositórios dele: some das listas e responde o mesmo 404 de um id que não existe. Nomes, descrições, versões e diagnósticos seguem a mesma regra, porque um título pode revelar um repositório que a pessoa não pode ler. Operadores e o administrador da plataforma são exceção, para poderem consertar o próprio workspace: um operador que perdeu o papel em A vê os repositórios que consegue ler e a contagem dos ocultos, e toda resposta do grafo continua filtrada pelo grant. Workspaces são identificados por um id determinístico e os nomes podem se repetir, então criar um não revela nada sobre os ocultos.

A perda de um papel ou da participação em um grupo remove o acesso no próximo conjunto de permissões e nunca é revivida por um conjunto mais antigo. O workspace, suas versões e seu histórico não são apagados.

## Contratos distintos

1. **Catálogo** (entregue): os repositórios que o GitHub App cobre e que a lista de permitidos do administrador admite, listados pelo job no mesmo conjunto de permissões dos papéis. Quem consulta vê apenas os repositórios que pode selecionar; cobertura não é direito.
2. **Entradas selecionadas**: um manifesto versionado de repositórios com peers, relações de provedor, referências a variáveis de ambiente, recursos e configuração de serve. Não leva caminho de mirror, valor secreto, owner nem lista de documentos.
3. **Ingestão compartilhada**: uma entrada por repositório entre todos os workspaces. Uma entrada em que nenhum operador do seu workspace tem um papel aceito é excluída antes do fetch, o que não é um fetch com falha. Uma falha de fetch ou de índice continua abortando a publicação inteira, e todo workspace mantém a última geração completa.
4. **Versões**: salvar, aplicar, reverter, renomear e remover, cada uma conferida em uma transação contra a versão base, a autoridade de quem a faz e a elegibilidade atual, e cada uma auditada.
5. **Seleção da consulta**: `/-/workspaces/<id>/<identity>` responde a partir do workspace como uma geração o registrou, limitado pelo manifesto e pelo grant. Uma geração fixada o reproduz exatamente. A rota simples `/<identity>` continua servindo o workspace do deployment.

## Configuração efetiva

O `meada.toml` do próprio repositório continua vencendo por inteiro sobre as declarações de qualquer workspace, e o editor mostra quando uma declaração é ignorada. As declarações para repositórios sem ele são por workspace: dois workspaces que compartilham Y podem declarar peers ou hosts de serve diferentes sem um segundo índice, porque são aplicadas ao responder e nunca entram no bundle. Quais documentos são indexados pertence à ingestão e é igual em todos os workspaces.

<!-- meada-graph:mcp-proposal -->

## Autoria assistida por MCP

Um servidor de propostas separado permite que um agente de CLI, usando o próprio conector de leitura do GitHub, liste os repositórios que o operador pode selecionar e pergunte se um manifesto é válido e autorizado. Ele deriva identidade e grants da própria autenticação, nunca de argumentos, não encaminha credencial alguma e trata tudo o que o agente envia como não confiável. Devolve o manifesto canônico, um diff, diagnósticos e a versão base, e não grava nada: nenhum rascunho, nenhuma versão, nenhum ponteiro movido. Uma pessoa cola a proposta no console, que a valida de novo e reconfere a base, a autoridade e os grants ao salvar e aplicar.

A separação entre [versão de workspace e geração](workspaces.md), e entre [admissão e grant](admission.md), continua essencial para ler este desenho.

## Ordem de entrega

Contratos, depois o catálogo elegível, o primeiro workspace, versões e sobreposição, e a configuração por contexto. A autoria guiada e as propostas por MCP avançam então em paralelo, e a validação empresarial depende de ambas. O [roadmap](roadmap.md) acompanha cada etapa.
