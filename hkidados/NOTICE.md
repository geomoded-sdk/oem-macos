# Créditos, origem e licenças

## HakiOS — o trabalho que este projeto reutiliza

- Repositório: <https://github.com/oosexgang/hakios-xnu-kernel>
- Commit fixado: `63fc2b4633bac3af8e57dadab9f5a8d4cb940438`
- Licença do repositório: MIT (arquivo `LICENSE` do próprio HakiOS).
- Kernel: XNU `xnu-8796.121.2`, com o patch da comunidade em `xnu/hakios.patch`.
  O XNU é da Apple, sob a Apple Public Source License 2.0
  (<https://opensource.apple.com/apsl/>). O patch e o port são do HakiOS.
- Componentes citados e não distribuídos aqui: `xnuloader`, `drivers/`,
  `tools/apfs/`, `sysimage/`, `userland/`, `prebuilt/`.

### Arquivos copiados verbatim para dentro deste projeto

| destino | origem | sha256 |
|---|---|---|
| `hkipados/vendor/hakios/apfs.py` | `tools/apfs/apfs.py` | `a051520d0c1f4f9d783c484be3beaeb536a1b4364fd8ed2e6a9b5c2d98592588` |
| `hkipados/vendor/hakios/dmg.py`  | `tools/apfs/dmg.py`  | `d7aab6e514a3b6a63cc666bd6129529e315e04fb2eff897b6333ce8d2dba8423` |
| `hkipados/vendor/hakios/lzbitmap/libzbitmap.c` | `tools/apfs/lzbitmap/libzbitmap.c` | `db3dbcac84d689b740f0cdb2e5d274dee9e51c22823cd49067ee362235df8af4` |
| `hkipados/vendor/hakios/lzbitmap/libzbitmap.h` | `tools/apfs/lzbitmap/libzbitmap.h` | `2181dc070a09df6b2eff6eccb143798725d2fb2b28875cf758ad5f77265dbd18` |
| `hkipados/vendor/hakios/lzbitmap/unzbm.c` | `tools/apfs/lzbitmap/unzbm.c` | `e283200251b2575ec13c453129373e926bb9eafe123224aa6dff01bbb5182f7a` |

O `lzbitmap/` tem `SPDX-License-Identifier: GPL-2.0+ OR MIT`, Copyright (C)
2022 Corellium LLC (Ernesto A. Fernández), portado de
<https://github.com/eafer/libzbitmap>. O HkiPadOS o usa sob a opção MIT, com o
cabeçalho original intacto.

`scripts/fetch-hakios.sh` confere esses hashes. Nada neles foi alterado; as
modificações do HkiPadOS vivem em `hkipados/apfsx.py`, `hkipados/apfs_sealed.py`
e `hkipados/decmpfs.py`, que os importam ou compilam.

## Apple

Nenhum arquivo da Apple é distribuído neste repositório. O IPSW é obtido pelo
usuário, da Apple, e fica em `cache/` (fora do controle de versão). "iPadOS",
"Darwin", "XNU", "APFS", "AEA" e "Apple" são citados para descrever com o que o
código trabalha. Este projeto não é afiliado nem endossado pela Apple.

## Ferramentas de terceiros

- **7-Zip** (Igor Pavlov, LGPL + unRAR restriction) — plano B, como processo
  externo, para imagens que o leitor próprio não abre. Não é distribuído aqui;
  `apt install 7zip`.
- **lzfse** (pacote Python) — descompressão LZFSE do kernelcache. Opcional.
- **QEMU** e **AAVMF/edk2** — usados no teste de boot aberto. Não distribuídos.

## O que o HkiPadOS acrescenta

Tudo em `hkipados/` (exceto `hkipados/vendor/`), `bin/`, `scripts/`, `tests/` e
`docs/` é do HkiPadOS, sob MIT (`LICENSE`).
