# Uso

Na raiz do repositório:

- `meada index` monta o índice, em `.meada/`, do repositório e dos pares;
- `meada status` mostra o conjunto de repositórios, o estado de cada índice e os pares que não
  foram encontrados; sob cada repositório, os recursos sem ligação e os `via` que não
  correspondem a nenhum símbolo, e cada arquivo-fonte que um language pack descartou por passar de um
  limite (`abrangente demais`, para o orçamento de nomes qualificados, ou um projeto que dá usings demais
  aos seus arquivos);
- `meada export` imprime o índice inteiro em JSON ordenado, bom para `diff` entre duas execuções;
- `meada decide` resolve ligações ambíguas e URLs dinâmicas, veja Decisões;
- `meada mcp` serve as [ferramentas MCP](mcp.md).

O export começa por três campos que dizem de onde vem. `format` nomeia o formato do export (1
hoje): sobe quando quem lê o formato anterior o leria errado (um campo renomeado ou removido,
um significado mudado), nunca por um campo opcional acrescentado, e não é a versão do esquema do
índice. `commit` é o commit para o qual `HEAD` apontava quando o índice foi lido, e `dirty` diz se
o checkout diferia dele (arquivos alterados, removidos ou não rastreados; os ignorados, como
`.meada/`, não contam). Os dois somem juntos fora do git, antes do primeiro commit e quando `HEAD`
ou os arquivos mudam durante a atualização. No modo servidor eles vêm do que `meada index
--workspace` registrou, ou da geração, nunca do `HEAD` atual do espelho, e lá `dirty` é sempre
`false`.

Meada só escreve dentro de `.meada/`, que se ignora no git. Toda consulta primeiro atualiza o
índice com o que mudou nos arquivos, inclusive o que ainda não foi commitado. As mensagens da
linha de comando seguem o `LANG` (português, inglês ou espanhol); as do MCP são sempre em inglês.
