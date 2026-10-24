# Uso

Na raiz do repositório:

- `meada index` monta o índice, em `.meada/`, do repositório e dos pares;
- `meada status` mostra o conjunto de repositórios, o estado de cada índice e os pares que não
  foram encontrados; sob cada repositório, os recursos sem ligação e os `via` que não
  correspondem a nenhum símbolo;
- `meada export` imprime o índice inteiro em JSON ordenado, bom para `diff` entre duas execuções;
- `meada decide` resolve ligações ambíguas e URLs dinâmicas, veja Decisões;
- `meada mcp` serve as [ferramentas MCP](mcp.md).

Meada só escreve dentro de `.meada/`, que se ignora no git. Toda consulta primeiro atualiza o
índice com o que mudou nos arquivos, inclusive o que ainda não foi commitado. As mensagens da
linha de comando seguem o `LANG` (português ou inglês); as do MCP são sempre em inglês.
