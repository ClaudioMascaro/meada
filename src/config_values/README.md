# Valores HCL e seletores de configuração

Valores de configuração em HCL e resolução de localizações de configuração citadas por declarações.

## Interface

O leitor HCL transforma atributos em linhas com caminho da chave e localização. O resolvedor recebe um seletor e procura as posições correspondentes no índice de um repositório. `[hcl.rs:read]` `[sites.rs:locate]` `[sites.rs:Site]`

## Invariantes

Valores nulos são omitidos. Os seletores exigem valores escalares completos e descartam textos truncados ou ocultos. `[hcl.rs:read]` `[sites.rs:locate]`

O leitor HCL rejeita o arquivo se o aninhamento entre chaves, colchetes e parênteses exceder 64 níveis. `[hcl.rs:NESTING_MAX]`

## Efeitos colaterais

O parser trabalha sobre os bytes fornecidos. A localização consulta o SQLite já aberto pelo chamador. `[hcl.rs:read]` `[sites.rs:locate]`

## Verificação

Na raiz do repositório, execute `cargo test -p meada --test it config_values::`.
