# Workspaces nomeados

!!! warning "Planejado: S5"
    [S5, #339](https://github.com/ClaudioMascaro/meada/issues/339) é escopo sequencial aceito após S3 e S4. A [consolidação dos contratos, #340](https://github.com/ClaudioMascaro/meada/issues/340), definirá os contratos finais do runtime. O console entregue ainda opera com um workspace por deployment.

## Compor sem duplicar a indexação

A proposta permite workspaces com nomes e repositórios sobrepostos, sobre a indexação compartilhada. O mesmo repositório não ganha outro índice por pertencer a dois workspaces. Haverá três papéis humanos: administrador da plataforma, operador de workspace e viewer.

O grant efetivo por repositório continuará sendo a autoridade. Um operador com acesso apenas a A poderá compor Y+A; outro com acesso apenas a B poderá compor Y+B. A presença compartilhada de Y não concederá A ao segundo nem B ao primeiro. A leitura de Y também dependerá do grant efetivo de cada sujeito.

Colegas do mesmo grupo herdarão o papel viewer sem outra aprovação do provedor de identidade, do Meada ou pacote de acesso. Esse papel continuará limitado pelos grants de repositório.

<!-- meada-graph:workspace-overlap -->

## Autoria assistida por MCP

A autoria autenticada por MCP será stateless e produzirá somente propostas. Não salvará nem aplicará configurações. A pessoa revisará a proposta e usará o fluxo autorizado do console para persistir e aplicar mudanças.

A separação entre [versão de workspace e geração](workspaces.md), e entre [admissão e grant](admission.md), continua essencial para ler essa proposta.

<!-- meada-graph:mcp-proposal -->
