# HkiPadOS

**Este diretório é um projeto separado.** Ele não faz parte do `oem-macos`, não o
substitui, não compartilha código com ele e tem licença, histórico e objetivos
próprios. Está aqui apenas porque o trabalho começou neste workspace.

O `oem-macos` é uma camada OEM sobre o `reims-vgpu`. O HkiPadOS é outra coisa:
reutilizar o **HakiOS** — o XNU portado pela comunidade para celulares Android —
para analisar e, se possível, iniciar a userland do iPadOS extraída de um IPSW
oficial.

Nada aqui foi copiado do `oem-macos`. Nada do `oem-macos` depende daqui.

---

## O que este projeto é

Três entregas, todas verificáveis:

1. **Pipeline de IPSW** — baixar, verificar, extrair e analisar um IPSW oficial
   sem depender de ferramenta externa para o essencial. Lê o `BuildManifest` por
   *HTTP Range* (185 KB, não 11,6 GB), verifica CRC32 de cada membro, lê IM4P,
   Mach-O, AEA e APFS.
2. **Reuso do HakiOS** — kernel, loader e drivers preservados e fixados por
   commit; os arquivos reutilizados são copiados verbatim e conferidos por
   SHA-256. O HkiPadOS só acrescenta ferramentas.
3. **Relatório de compatibilidade** — cada afirmação sobre o que funciona vem de
   um teste executado, com comando e saída em `docs/07-testes.md`.

## O que este projeto não é

- Não é um iPadOS e não tem interface falsa: sem GTK, Android, launcher ou tema.
- Não é um kernel novo: o kernel é o do HakiOS (`xnu-8796.121.2` + patch).
- Não contorna assinatura, ativação, SEP, AEA ou qualquer proteção.
- Não declara compatibilidade de hardware que não foi testada.

## Alvo

| | |
|---|---|
| Dispositivo | iPad13,4 — iPad Pro 11" (3ª geração, M1), board `j517ap`, `t8103` |
| Sistema | iPadOS **27.2.1 (24B156)**, publicado em 2026-11-09 |
| IPSW | `iPad_Pro_Spring_2021_27.2.1_24B156_Restore.ipsw` |
| Tamanho | 11.655.546.609 bytes |
| SHA-256 | `b00bb0cbc17a810ebd36bb4befa6cc7599c1abdbf92093abf2a4db2d8aebe333` |

## Resultado, sem enfeite

**O que funcionou, medido neste ambiente:**

- o IPSW foi baixado, o SHA-256 e os 185 CRCs conferem;
- o kernel do alvo foi lido: `MH_FILESET`, arm64e, chained fixups,
  `xnu-13432.42.3`, Darwin 27.2.0, 76.775.424 bytes descomprimidos;
- o DeviceTree do alvo foi descomprimido (260.176 bytes) e diz o hardware que
  existe: `aic,1`, `dart,t8020`, `ans`, `gpu,t8103`, `dcp`, `sep`, `ane,t8020`;
- duas imagens não cifradas foram extraídas **pelo leitor próprio**, sem 7-Zip:
  **1.155 arquivos** do cryptex de App (volume APFS selado, 5.023 hashes de nó
  conferidos) e **1.260** do RestoreRamDisk, com **387 Mach-O arm64e** —
  inclusive o `launchd` de iOS 27.2.0 e seus 32 entitlements;
- os **869** arquivos do cryptex, todos `decmpfs` tipo 14 (LZBITMAP), foram
  **descomprimidos** com o `libzbitmap.c` do HakiOS: 18.422.104 bytes, 0 falhas,
  idênticos ao que o 7-Zip mais o `unzbm` produzem;
- o kernel comunitário **iniciou em QEMU**: `XNULOADER v3` → Darwin 22.5.0 →
  `bsd_init: done` → processo 1 imprimindo `Hello from user space on Darwin!`
  (`reports/qemu-open.log`);
- os 35 testes passam (4 pulados sem o IPSW).

**O que não funcionou, e por quê:**

- a interface do iPadOS não inicia. O volume `OS` (8,3 GB) é AEA e a chave vem do
  `wkms` da Apple para um aparelho autenticado; o `launchd` do alvo é arm64e de
  2026 e o kernel comunitário é arm64 de 2022; e o iPad M1 não tem o caminho de
  boot (nem os drivers AIC/DART/ANS/DCP/AGX/touch) que o HakiOS usa.

O detalhamento, requisito por requisito, está em
[`docs/04-compatibilidade.md`](docs/04-compatibilidade.md). As limitações e o que
as destravaria, em [`docs/08-limitacoes.md`](docs/08-limitacoes.md).

## Começar

```sh
make doctor                   # o que falta nesta máquina

# o IPSW (nada é baixado sem você pedir)
./bin/hkidados fetch --device iPad13,4 --build 24B156 --out cache/
./bin/hkidados verify cache/*.ipsw --crc

# análise completa, sem extrair os 11 GB
./bin/hkidados analyze cache/*.ipsw --json reports/ipad13,4-24B156.json

# só o que é legível, com inventário
./bin/hkidados userland cache/*.ipsw --out build/userland --json reports/userland.json

# dependências de cada binário: o que a árvore cobre e o que só o dyld cache tem
./bin/hkidados deps build/userland/restore-ramdisk --json reports/deps.json

# o veredito (sai com código 1 quando algo obrigatório está bloqueado)
./bin/hkidados compat cache/*.ipsw --userland build/userland --md reports/compat.md

# o kernel do HakiOS no QEMU (não usa o IPSW)
make boot-open

# testes
make test
```

`make test` não precisa do IPSW. O teste de integração só roda com
`HKIPADOS_IPSW=cache/...ipsw`.

## Estrutura

```
hkidados/
├── bin/hkidados               o executável
├── hkipados/                  o HkiPadOS (MIT)
│   ├── source.py              leitura local e HTTP Range
│   ├── zipx.py                ZIP64 sobre qualquer fonte
│   ├── manifest.py            BuildManifest.plist
│   ├── im4p.py                IM4P/IM4M (img4), LZFSE
│   ├── macho.py               arm64 vs arm64e, plataforma, entitlements
│   ├── aea.py                 cabeçalho de Apple Encrypted Archive
│   ├── apfsx.py               APFS cru, IM4P e UDIF: tira o embrulho
│   ├── apfs_sealed.py         árvore APFS selada (hash) e árvore de extents
│   ├── decmpfs.py             LZBITMAP: compila o do HakiOS e descomprime
│   ├── deps.py                dependências dos Mach-O contra a árvore extraída
│   ├── analyze.py             o relatório
│   ├── compat.py              as regras de compatibilidade
│   ├── cli.py
│   └── vendor/hakios/         cópia verbatim do HakiOS + PROVENANCE.md
├── scripts/                   clone do HakiOS, boot em QEMU
├── docs/                      01..08 (ver abaixo)
├── reports/                   saídas reais: JSON e logs
└── tests/                     unittest (35 testes)
```

### Documentação

| | |
|---|---|
| [01-fontes](docs/01-fontes.md) | HakiOS, IPSW, licenças, o que é de quem |
| [02-arquitetura](docs/02-arquitetura.md) | o pipeline e por que cada peça existe |
| [03-analise-ipsw](docs/03-analise-ipsw.md) | o que o IPSW contém, medido |
| [04-compatibilidade](docs/04-compatibilidade.md) | hardware do alvo e os bloqueios |
| [05-userland](docs/05-userland.md) | o que foi extraído, e o que não foi |
| [06-boot](docs/06-boot.md) | a cadeia do iPad e o boot real em QEMU |
| [07-testes](docs/07-testes.md) | comandos, resultados, o que quebrou |
| [08-limitacoes](docs/08-limitacoes.md) | o que falta e o que destravaria |

## Créditos e licenças

O trabalho pesado é da comunidade **HakiOS**
(<https://github.com/oosexgang/hakios-xnu-kernel>): o port do XNU, o loader, os
drivers, o leitor APFS, o ramfs e o Metal sobre Adreno. Sem ele este projeto não
existiria. `NOTICE.md` lista os arquivos reutilizados, o commit exato e os
SHA-256.

- Código do HkiPadOS: MIT (`LICENSE`).
- HakiOS: MIT. XNU: Apple Public Source License 2.0.
- Nenhum arquivo da Apple é distribuído aqui. O IPSW é obtido por você, da Apple,
  e não é versionado (`cache/` está no `.gitignore`).

## Aviso

Não é afiliado nem endossado pela Apple. "iPadOS", "Darwin", "XNU" e "Apple" são
citados para descrever com o que o código trabalha. Não há suporte a ativação,
Apple ID, iCloud ou App Store, e não se tenta obtê-lo.
