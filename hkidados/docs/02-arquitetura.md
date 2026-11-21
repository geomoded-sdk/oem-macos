# 2. Arquitetura

## 2.1 A separação que importa

```
hkidados/
├── hkipados/vendor/hakios/     cópia verbatim do HakiOS (MIT) — não se edita
├── hkipados/                   o HkiPadOS (MIT)
├── upstream/                   o PIN e o que se espera do HakiOS
├── scripts/                    integração com o HakiOS (clone, boot em QEMU)
├── docs/                       este material
├── reports/                    saídas reais (JSON e logs)
└── tests/                      unittest
```

A regra é simples: **o código comunitário nunca é alterado no lugar**. O que o
HkiPadOS precisa acrescentar fica em um módulo próprio que importa o original.
Hoje há um caso: `hkipados/apfsx.py` usa `hkipados/vendor/hakios/apfs.py` e diz
onde ele para; não corrige o arquivo por baixo.

## 2.2 O pipeline

```
                 URL (ipsw.me → CDN)                 arquivo local
                          │                                │
                          └────────── source.py ───────────┘
                                 (LocalSource | HttpRangeSource)
                                            │
                                      zipx.py  (ZIP64, CRC32)
                                            │
        ┌───────────────────────────────────┼──────────────────────────────┐
        │                                   │                              │
  BuildManifest.plist                membros IM4P                  imagens (.dmg/.aea)
        │                                   │                              │
   manifest.py                        im4p.py + macho.py          aea.py / apfsx.py
        │                                   │                              │
        └──────────────► analyze.py ◄───────┘                              │
                              │                                             │
                       reports/*.json                             7-Zip (sealed APFS)
                              │                                             │
                        compat.py ◄─────────────────────────────────────────┘
                              │
                     reports/compat.md
```

### Por que HTTP Range

O `BuildManifest.plist` ocupa 185.172 bytes comprimidos (3.092.175 descomprimidos)
e descreve todos os 185 membros. Ler só ele custa ~185 KB de rede, não 11,6 GB. O `HttpRangeSource` exige
`Accept-Ranges: bytes`; se o servidor devolver 200 com o arquivo inteiro, ele
falha em vez de baixar tudo por acidente. O teste `test_refuses_a_server_that_ignores_range`
cobre isso com um servidor HTTP real.

### Por que o ZIP é reimplementado

O `zipfile` da biblioteca padrão não lê de uma fonte HTTP. O `ZipArchive` lê o
diretório central (17 KB), que é a única parte que descreve todos os membros, e
busca os bytes de cada membro sob demanda. É o mesmo código para arquivo local e
para URL, e o CRC32 do diretório central é verificado na extração.

### O volume selado e o 7-Zip

O volume do cryptex de App é APFS **selado**. O leitor do HakiOS verifica
checksum Fletcher-64 em todo bloco e ordena a árvore por object-id; a árvore
selada não tem checksum no nó (cabeçalho zerado, `BTNODE_NOHEADER`) e cada filho
carrega o SHA-256 do nó (`APFS_BTREE_HASHED`). Além disso os extents ficam numa
árvore separada (`apfs_fext_tree_oid`).

O `apfs_sealed.py` cobre exatamente esses dois pontos, seguindo o `apfs_lib.c` do
APFS da Apple, e reusa o leitor do HakiOS para tudo o mais. Com isso o cryptex é
lido e escrito **sem o 7-Zip** — 1.155 arquivos, 18.422.104 bytes, idênticos ao
que o 7-Zip produz. O 7-Zip continua no projeto como plano B (e para formatos que
não são APFS), e o relatório diz qual dos dois leu.

## 2.3 O que cada módulo garante

| módulo | garantia | teste |
|---|---|---|
| `source.py` | lê intervalo exato; recusa servidor sem Range | `test_io.py` (servidor HTTP real) |
| `zipx.py` | ZIP64; CRC32 e tamanho conferem, ou erro | `test_io.py` |
| `manifest.py` | identidade por produto/variante; digest pelo tamanho | `test_formats.py` |
| `im4p.py` | formato embrulhado (27) e clássico; LZFSE com erro explicado | `test_formats.py` |
| `macho.py` | arm64 vs arm64e; plataforma; minos; chained fixups; entitlements | `test_formats.py` |
| `aea.py` | lê o cabeçalho e diz que a chave vem do wkms | `test_formats.py` |
| `apfsx.py` | distingue APFS cru, IM4P e UDIF; tira o embrulho | `test_formats.py` |
| `apfs_sealed.py` | lê a árvore selada (NOHEADER + HASHED) e a árvore de extents | `test_apfs_sealed.py` |
| `decmpfs.py` | compila o LZBITMAP do HakiOS e descomprime tipo 14 na memória | `test_apfs_sealed.py` |
| `compat.py` | uma linha por requisito, com motivo e evidência | `test_compat.py` |

## 2.4 O que o pipeline não faz

- Não decifra AEA. Não tenta. A chave vem do `wkms` da Apple, autenticada pelo
  dispositivo; obtê-la seria contornar a proteção.
- Não decifra o volume `OS` (AEA) nem monta o sistema selado de 8,3 GB.
- Não escreve em APFS: lê o volume e extrai para o sistema de arquivos do host.
- Não inicia a userland do iPadOS. O relatório de compatibilidade explica por quê,
  requisito por requisito.

## 2.5 A integração com o kernel comunitário

O HkiPadOS não reescreve o kernel. Ele:

1. fixa o commit do HakiOS e confere o SHA-256 dos arquivos reutilizados
   (`scripts/fetch-hakios.sh`);
2. monta o payload do loader com os binários `prebuilt/` e inicia em QEMU
   (`scripts/hakios-boot-open.sh`), provando que o kernel comunitário funciona;
3. mede o que a userland do iPadOS exige e compara com o que o kernel oferece
   (`compat.py`).

O ponto 3 é o que dá o veredito. O ponto 2 é o que impede o veredito de ser
"não testamos nada".
