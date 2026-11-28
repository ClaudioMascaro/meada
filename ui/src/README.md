# Aplicação do console

Páginas React, cliente HTTP e integração com o núcleo WASM.

## Interface

A inicialização carrega a configuração, escolhe o modo de autenticação e entrega o cliente da API à aplicação. `[main.tsx:start]` `[App.tsx:App]` `[api.ts:Api]`

## Invariantes

O callback de autenticação remove o código da URL antes da descoberta e da troca. Uma origem diferente da configurada provoca redirecionamento no fluxo com emissor. `[main.tsx:start]` O callback consome o verificador PKCE e exige que o parâmetro `state` coincida com o salvo na aba. `[auth.ts:callback]`

## Efeitos colaterais

A interface envia requisições HTTP, usa armazenamento de sessão por aba e renderiza os resultados no DOM. `[api.ts:Api]` `[main.tsx:start]` `[local.ts:LocalAuth]`

## Verificação

Na raiz do repositório, execute `npm --prefix ui run typecheck && npm --prefix ui test`.
