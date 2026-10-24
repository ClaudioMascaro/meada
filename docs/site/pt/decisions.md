# Decisões

Uma ligação ambígua (vários provedores servem a rota) ou uma URL dinâmica (a chamada monta a
URL em tempo de execução) fica como item aberto. A ferramenta `search` do MCP, com o tipo
`ambiguous` ou `unresolved`, lista esses itens com o número de candidatos de cada um, não os
candidatos; `meada decide --list` lista os itens com os candidatos. Quem resolve é você, no
modo humano, ou um modelo, se você pedir.

- `meada decide` mostra cada item aberto do repositório, com os candidatos numerados; você
  escolhe um número, ou `s` para pular. Ao fim grava as escolhas em `meada.decisions.toml`, na
  raiz. Não escolheu nada, não grava.
- `--item <id>` trata só aquele item (o id que `meada decide --list` ou a ferramenta `search`
  imprime), mesmo que já decidido, para refazer a decisão; `--dry-run` imprime o arquivo que
  seria gravado e não escreve; `--list` imprime os itens abertos, mais as
  decisões obsoletas, e não pergunta nada; `--prune` remove as decisões obsoletas (não combina
  com as três anteriores).
- `--decider` troca a pessoa por um modelo (veja abaixo); `--show-request` (só com `--decider`)
  imprime cada requisição que seria enviada e não envia nenhuma; `--min-p <p>` (só com
  `--decider`, de 0 a 1) é a probabilidade mínima para o modelo gravar.

`meada.decisions.toml` é um arquivo versionado no git, como um lockfile: o diff dele é o que se
revisa. Cada `[[decision]]` traz o tipo (`kind`), a chamada (`call`), o provedor escolhido (`target`), quem
decidiu (`by`: `human` ou o id do modelo) e, para um modelo, a probabilidade (`p`). A
indexação só relê o arquivo: a ligação decidida ganha o nível `decided` e o motivo mostra quem
decidiu. Quando o código muda e uma decisão deixa de caber (a chamada sumiu, o item não está
mais aberto, o provedor sumiu ou não é mais candidato), `meada status` a mostra como obsoleta,
sob o repositório, e `meada decide --prune` a remove. Uma decisão que espera um par ainda não
encontrado aparece à parte e é mantida.

## Referência de `meada.decisions.toml`

O arquivo só aceita uma lista `[[decision]]`; ausente ou vazia significa nenhuma decisão.
Não há campos de versão, horário ou configuração de modelo. Cada entrada contém:

| Chave | Tipo | Regra |
| --- | --- | --- |
| `kind` | string obrigatória | `ambiguous-link` para vários provedores candidatos, ou `dynamic-url` para URL não avaliada. |
| `call` | string obrigatória | Identidade canônica da chamada mostrada no export; só uma entrada por `call` em todo o arquivo, independentemente do tipo. |
| `target` | string obrigatória | `<identidade-do-provedor>::<contrato>`, o alvo escolhido. |
| `by` | string obrigatória | Exatamente `human` para pessoa; outro valor identifica o modelo. |
| `p` | número de ponto flutuante, condicional | Proibido com `by = "human"`; obrigatório para modelo, finito e entre 0 e 1, inclusive. |

Por exemplo, uma escolha humana:

```toml
[[decision]]
kind = "ambiguous-link"
call = "example.com/acme/web::src/orders.ts::load::fetch#0"
target = "example.com/acme/api::http::GET::/orders/{}"
by = "human"
```

Uma decisão de modelo usa `by = "<id-do-modelo>"` e acrescenta, por exemplo, `p = 0.9`.
Chaves desconhecidas, tipos errados e chamadas repetidas dão erro com arquivo/linha. A
validade semântica de um alvo é verificada ao reaplicar as decisões: um alvo que sumiu pode
tornar a decisão obsoleta, em vez de ser um erro de sintaxe TOML. O escritor ordena por
`call` e não preserva comentários nem acrescenta timestamps.

### Decisor por modelo

`meada decide --decider` é o único caminho de Meada até um modelo de linguagem, e só roda quando você o pede.
Quatro variáveis de ambiente o configuram (nada vem de arquivo):

| Variável | Papel | Padrão |
| --- | --- | --- |
| `MEADA_DECIDER_PROTOCOL` | `decisions` ou `chat` | `decisions` |
| `MEADA_DECIDER_URL` | URL base | `https://openrouter.ai` (`decisions`); obrigatória com `chat` |
| `MEADA_DECIDER_KEY` | chave da API, enviada só no cabeçalho `Authorization` | obrigatória com `decisions`; opcional com `chat` |
| `MEADA_DECIDER_MODEL` | modelo | `typesafe/jev-1.14` (`decisions`); obrigatória com `chat` |

- **`decisions`** (o padrão) usa a API de decisões do OpenRouter, com o Jev: uma única
  requisição leva todos os itens, e a resposta traz a probabilidade de cada candidato. A
  requisição pede retenção zero de dados (`zdr: true`, `data_collection: deny`); se o
  OpenRouter recusar essa preferência, a requisição falha e nada é gravado.
- **`chat`** fala com qualquer endpoint compatível com a API de chat da OpenAI
  (`<URL>/chat/completions`), inclusive um modelo local, por exemplo pelo Ollama:
  `MEADA_DECIDER_PROTOCOL=chat MEADA_DECIDER_URL=http://localhost:11434/v1
  MEADA_DECIDER_MODEL=<modelo> meada decide --decider`. Uma requisição por item. A
  probabilidade é a que o próprio modelo diz de si, não uma medida calibrada.

Qualquer gateway que fale um dos dois protocolos serve: basta trocar a URL. HTTPS em qualquer
lugar; HTTP só para `localhost` ou um endereço de loopback.
Valores vazios dessas variáveis contam como ausentes; uma chave precisa conter apenas
caracteres ASCII visíveis. Não coloque a chave no `meada.toml` nem no arquivo de decisões.

O que sai da máquina, por item: a chamada, a função que a contém (lida do disco) e a lista de
candidatos (repositório, método e caminho), mais o id do repositório que a chama. Nenhum
código do provedor, nenhum outro arquivo. `--show-request` mostra tudo isso antes, com a chave
trocada por `<hidden>`. Cada resposta é validada por item; uma inválida ou uma requisição
que falhou é relatada sem gravar aquele item, e o comando sai com erro.

Constantes fixas:

- `--min-p` padrão de 0,5: abaixo disso a proposta é impressa como sugestão e não é gravada.
  Muda só por um PR com dados.
- No máximo 50 candidatos por item: com mais, o item é pulado no modo decisor (o modo humano
  ainda o oferece).
- 120 segundos por requisição, da conexão ao último byte, sem tentar de novo.
- No protocolo `decisions`, as probabilidades de uma resposta devem somar 1 com tolerância de
  0,01.
