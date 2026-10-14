# Meada

**Português (Brasil)** · [English](README.en.md) · [Español](README.es.md)

Meada lê os seus repositórios, monta o mapa de quem chama quem e costura um repositório no
outro pelos contratos que eles compartilham: rotas HTTP, filas, pacotes, variáveis de ambiente.
Agentes de código, e gente também, consultam esse mapa por MCP. Nenhuma etapa da indexação usa
LLM.

> **Estado:** em construção. Ainda não há nada para instalar. As decisões tomadas até aqui
> estão em [`docs/adr`](docs/adr).

## Por que mais uma ferramenta

Quem trabalha com vários serviços sabe que o bug quase nunca mora num repositório só. O front
chama uma rota que mudou no back; o worker consome uma fila que outro time renomeou. Das
ferramentas de grafo de código que existem hoje, algumas olham um repositório por vez, outras
só ligam repositórios quando o texto da rota bate exatamente, e outras proíbem uso comercial.

Meada quer seguir o fio de ponta a ponta, atravessando quantos repositórios for preciso, e dizer
com que confiança cada ligação foi feita.

## Princípios

- **O seu código fica com você.** Roda na sua máquina, sem conta, sem chave de API e sem
  telemetria.
- **Sem LLM para indexar.** O mapa sai da árvore sintática do código. Não tem conta em dólar
  por token, funciona offline e cabe num notebook comum.
- **Português primeiro.** A documentação nasce em português do Brasil; inglês e espanhol são
  traduções. Issues e PRs podem ser abertos em qualquer uma das três línguas. O código é
  escrito em inglês para que qualquer pessoa consiga ler.
- **Juntar sem misturar.** "Nem tudo que se ajunta se mistura", ensinou Antônio Bispo dos
  Santos, o Nêgo Bispo. Cada repositório guarda o próprio índice; Meada só costura as pontas.
- **Pouco ruído para o agente.** Meada não escreve no seu `CLAUDE.md`, não instala hooks e não
  mexe na configuração do editor sem um comando seu. As respostas são curtas.
- **Crédito a quem veio antes.** Meada aprende com GitNexus, CodeGraph, codebase-memory-mcp e
  kivgraph. O que cada um faz está em [`docs/prior-art.md`](docs/prior-art.md).

## Como participar

O projeto está começando, e o escopo da primeira versão vai ser decidido em aberto. Já dá para
ajudar:

- abrindo uma issue com o seu caso de uso entre repositórios, em português, español ou English;
- revisando as traduções: se o inglês ou o espanhol soarem estranhos, a correção é bem-vinda;
- lendo e questionando os ADRs.

## O nome

Meada é uma porção de fios enrolados. "Puxar o fio da meada" é achar a ponta que desfaz o nó,
e é isso que a ferramenta tenta fazer quando segue uma chamada de um repositório a outro.

## Licença

[Apache-2.0](LICENSE).
