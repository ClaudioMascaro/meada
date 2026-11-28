# Build do site

Geração e validação do site multilíngue do Meada.

## Interface

Os hooks do MkDocs validam os grafos, geram SVGs e transformam os marcadores das páginas. O verificador lê o HTML publicado e os links dos arquivos de origem. `[hooks.py:on_pre_build]` `[hooks.py:on_page_markdown]` `[check.py:source_links_check]`

## Invariantes

O bundle reúne as licenças dos pacotes usados e o build falha quando falta um aviso de licença. `[build.mjs:notices]`

## Efeitos colaterais

A geração grava SVGs, JavaScript, CSS e avisos de terceiros nos assets do site. SVGs gerados que saem do conjunto de diagramas são removidos. `[hooks.py:on_pre_build]` `[build.mjs:writeFile]`

## Verificação

Na raiz do repositório, execute `mkdocs build --strict && python scripts/docs/check.py`.
