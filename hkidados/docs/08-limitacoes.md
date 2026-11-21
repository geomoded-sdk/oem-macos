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
| arm64 (HakiOS) × arm64e (alvo) | PAC/`ptrauth` não suportado | port para arm64e | parcialmente |
| boot assinado no iPad | nada executa sem a Apple | quebrar o secure boot (não fazemos) | não |
| drivers AIC/DART/ANS/DCP/AGX/touch | sem tela, sem disco, sem entrada | engenharia reversa de cada um | parcialmente |
| leitor APFS do HakiOS e volume selado | só o 7-Zip lê o cryptex | árvore por hash no leitor | **sim** |
| `decmpfs` tipo 14 (LZBITMAP) | arquivos do cryptex com 0 byte | decodificador LZBITMAP | **sim** |
| kernel não recompilado aqui | não sei se o build local funciona | toolchain do theos (~12 GB) | **sim** |

## 8.3 O que dá para avançar sozinho

Três coisas, em ordem de valor:

1. **Decodificador LZBITMAP.** O formato é `ZBM` + bitmap + LZVN. Com ele, os 869
   arquivos do cryptex de App passam a ter conteúdo — e dá para comparar o
   `launchd`/`MobileSafari` reais com o que o kernel comunitário espera.
2. **Árvore APFS selada no leitor do HakiOS.** O `apfs.py` já sabe ler a árvore
   por object-id; falta a variante por hash (`BTNODE_NOHEADER`, filhos de 32
   bytes). Isso tiraria a dependência do 7-Zip e permitiria ler o volume selado
   em qualquer plataforma.
3. **Repetir tudo com o IPSW 16.5 que o HakiOS já usa.** É o único caso em que
   kernel e userland são da mesma geração; se algo for possível, é ali.

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
