# Arquitetura inicial

## Evidências do Explorer++ disponível

O diretório `C:\Users\Ednal\Downloads\explorerpp_x64` contém apenas `Explorer++.exe` (versão 1.4.0.2293), `Readme.txt`, `History.txt`, `License.txt` e ajuda compilada. Não há fontes C++ para inspecionar. O README descreve um gerenciador portátil para Windows; o histórico mostra abas, bookmarks organizados em árvore, colunas, ícones e integração de metadados do Recycle Bin (incluindo data de exclusão e localização original). Isso indica requisitos comportamentais, não permite afirmar como as estruturas internas ou chamadas Win32/Shell foram implementadas.

O binário declara GPL-3.0 no arquivo de licença. O novo workspace inicia independente, sob MIT OR Apache-2.0, com implementação própria baseada nos requisitos observáveis. Antes de reutilizar qualquer código ou ativo do Explorer++, será necessário obter e revisar as fontes e obrigações da licença. Não copiar implementação, recursos ou texto de ajuda do binário.

## Workspace proposto

```text
crates/
  file-engine/       filesystem, metadados, operações e busca
  platform-windows/  adaptador Win32/Shell (ícones, lixeira, volumes)
  app-core/          casos de uso, estado de abas e comandos da aplicação
apps/
  desktop/           UI nativa egui e composição do aplicativo
```

O `file-engine` é o núcleo e `apps/desktop` entrega a interface nativa em Rust/egui. A separação de shell em adaptador Windows mantém o motor portátil e evita misturar UI, regras de negócio e detalhes de plataforma. A UI oferece abas, navegação por histórico, favoritos, lista/grade, filtro por nome, painel duplo com seleção ativa, seleção múltipla, operações comuns e preferências persistentes. Plugins escritos pelo usuário em Rhai rodam sem bindings de arquivos/rede/processos e retornam comandos verificados por uma allowlist no host. Recomenda-se manter o núcleo comum permissivo (MIT/Apache-2.0) e tratar integrações/serviços Pro como crates/plugins opcionais; validar compatibilidade de licenças das dependências e distribuir avisos/fontes exigidos por suas licenças. Um sistema de features não substitui controle de acesso para produto pago.

## Dependências e responsabilidades

- `tokio` (features `fs`, `macros`, `rt-multi-thread`): APIs assíncronas de arquivos e runtime.
- `serde`: modelos serializáveis para IPC.
- `thiserror`: erros tipados.
- `eframe`/`egui`: shell e UI nativos escritos em Rust, usados em `apps/desktop`.
- `rfd`: seletor nativo de pastas.
- `rhai`: scripts de extensão sem acesso nativo ao sistema, com limite de operações e comandos validados pelo host.
- `directories` + `serde_json`: configurações e plugins em pastas de dados do usuário.
- `trash`: envio de itens à lixeira do sistema.
- `open`: abertura de arquivo pelo aplicativo padrão.
- `walkdir` somente para travessia recursiva; não é usado na listagem de um único diretório.
- Avaliar `notify` para atualização incremental, `ignore` para filtros de busca e `windows` crate para APIs Shell/Win32 isoladas no adaptador.

## Primeiro módulo

`file_engine::list_directory` lista um nível, sem seguir links simbólicos ao consultar metadados. Até 64 consultas de metadados ocorrem em paralelo; falhas ao abrir/percorrer o diretório são erros tipados e falhas por item ficam anexadas à entrada. A UI executa a listagem em threads Tokio, sem bloquear o loop gráfico. Cópia e movimentação recursiva rodam em worker e não substituem itens existentes. Ainda faltam paginação para pastas gigantes, progresso/cancelamento de tarefas e cobertura de teste antes de tratar o app como pronto para distribuição.
