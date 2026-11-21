# 8. Limitações e o que viria depois

## 8.1 O objetivo final não foi atingido, e por quê

O objetivo era: XNU do HakiOS + userland do iPadOS + interface original. O que
existe hoje é:

- o XNU do HakiOS **iniciando** (em QEMU, com o `hello` como processo 1);
- a userland do iPadOS **extraída e medida** nas duas imagens que não são AEA;
- um relatório que diz, requisito por requisito, onde a coisa para.

A interface do iPadOS não inicia. Não é uma questão de esforço: o volume que a
contém está cifrado com uma chave que só o aparelho obtém, e o kernel que a
executaria (`xnu-13432.42.3`, arm64e) não é público. Dizer o contrário seria
inventar resultado.

## 8.2 Cada limitação, com o que a destravaria

| limitação | impacto | o que destravaria | é software nosso? |
|---|---|---|---|
| AEA do volume `OS` (8,3 GB) | sem `SpringBoard`, sem frameworks | a chave do `wkms` | não |
| AEA do `SystemOS` (2,4 GB) | sem o sistema selado | idem | não |
| SEP substituído por stubs no HakiOS | `launchd` e `amfid` falham | SEP real | não |
| kernel 8796 × userland 13432 | traps e syscalls faltando | Apple publicar o 13432 | não |
| arm64 (HakiOS) × arm64e (alvo) | `EBADARCH` (errno 86) no `exec` do init; medido em `docs/06` §6.5 | port para arm64e | parcialmente |
| boot assinado no iPad | nada executa sem a Apple | quebrar o secure boot (não fazemos) | não |
| drivers AIC/DART/ANS/DCP/AGX/touch | sem tela, sem disco, sem entrada | engenharia reversa de cada um | parcialmente |
| kernel não recompilado aqui | não sei se o build local funciona | toolchain do theos (~12 GB) | **sim** |

### Duas dessas linhas foram medidas, não deduzidas

`scripts/try-ipados-init.sh` põe um binário do iPadOS em `md0` como processo 1 do
kernel do HakiOS e guarda o que o kernel diz:

- o `hello` do HakiOS roda; o **mesmo** binário com o cpusubtype trocado para
  arm64e recebe `errno 86` (`EBADARCH`) — logo o kernel recusa o ABI, não o
  binário;
- `sbin/launchd` do ramdisk recebe o mesmo `errno 86`;
- uma fatia arm64 com cpusubtype zerado passa do `EBADARCH` e morre em
  `namespace 9 code 0x1` = `OS_REASON_EXEC` / `EXEC_EXIT_REASON_BAD_MACHO`.

## 8.3 O que dava para avançar sozinho — e foi feito

Três itens dependiam só de software nosso. Dois estão fechados:

1. **Decodificador LZBITMAP — feito.** O `libzbitmap.c` que o HakiOS já trazia
   foi compilado (`libzbitmap.so`, chamado por `ctypes`) e o laço de blocos do
   `unzbm.c` foi portado para o `decmpfs.decompress`. Os **869 fluxos** do
   cryptex de App descomprimem, com o tamanho de cada bloco conferido. Prova:
   `tests/test_apfs_sealed.py` e a comparação byte a byte com o `unzbm`.
2. **Árvore APFS selada — feito.** O `apfs_sealed.py` lê a árvore com
   `APFS_BTREE_NOHEADER` + `APFS_BTREE_HASHED` e a árvore de extents
   (`apfs_fext_tree_oid`), seguindo `apfs_lib.c` da Apple. O cryptex sai inteiro
   sem o 7-Zip, com **5.023 hashes de nó conferidos** e resultado idêntico ao do
   7-Zip. O ramdisk, que é APFS comum embrulhado em IM4P, também sai inteiro.
3. **Repetir tudo com o IPSW 16.5 que o HakiOS já usa — continua aberto.** É o
   único caso em que kernel e userland são da mesma geração; se algo for
   possível, é ali.

O que sobrou na tabela do 8.2 não é código nosso: depende da Apple (assinatura,
SEP, AEA) ou de drivers para hardware que não existe neste ambiente.

## 8.4 O que este projeto deliberadamente não faz

- não tenta obter a chave do AEA;
- não tenta instalar nada no iPad;
- não usa jailbreak, exploit ou `checkm8`-like;
- não apresenta um `launchd` de 2022 como se fosse o de 2026;
- não desenha uma interface para "parecer" iPadOS.

## 8.5 Riscos de se levar adiante

- Um port do XNU 13432 para arm64e/AIC/DART é trabalho de anos e depende de código
  que a Apple não publicou.
- Qualquer avanço em AEA/SEP tem implicação legal e vai contra o pedido explícito
  de respeitar as proteções.
- O caminho realista é o que o HakiOS já faz: rodar a userland **da geração do
  kernel** em hardware que ele suporta, e usar o HkiPadOS para medir e documentar
  a distância até o iPadOS.
