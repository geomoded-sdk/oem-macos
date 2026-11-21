# Proveniência

Estes arquivos são cópias **verbatim** do projeto HakiOS. Não foram editados.

| arquivo | origem | sha256 |
|---|---|---|
| `apfs.py` | `tools/apfs/apfs.py` | `a051520d0c1f4f9d783c484be3beaeb536a1b4364fd8ed2e6a9b5c2d98592588` |
| `dmg.py`  | `tools/apfs/dmg.py`  | `d7aab6e514a3b6a63cc666bd6129529e315e04fb2eff897b6333ce8d2dba8423` |
| `lzbitmap/libzbitmap.c` | `tools/apfs/lzbitmap/libzbitmap.c` | `db3dbcac84d689b740f0cdb2e5d274dee9e51c22823cd49067ee362235df8af4` |
| `lzbitmap/libzbitmap.h` | `tools/apfs/lzbitmap/libzbitmap.h` | `2181dc070a09df6b2eff6eccb143798725d2fb2b28875cf758ad5f77265dbd18` |
| `lzbitmap/unzbm.c`      | `tools/apfs/lzbitmap/unzbm.c`      | `e283200251b2575ec13c453129373e926bb9eafe123224aa6dff01bbb5182f7a` |

- Repositório: <https://github.com/oosexgang/hakios-xnu-kernel>
- Commit: `63fc2b4633bac3af8e57dadab9f5a8d4cb940438`
- Licença: MIT, conforme o cabeçalho do repositório (`LICENSE`). O `apfs.py` lê
  estruturas descritas em `xnu/files/bsd/miscfs/apfs/apfs_format.h`, que é APSL 2.0.
- O `lzbitmap/` é `SPDX-License-Identifier: GPL-2.0+ OR MIT`, Copyright (C) 2022
  Corellium LLC, autor Ernesto A. Fernández, portado de
  <https://github.com/eafer/libzbitmap>. O HkiPadOS escolhe a opção **MIT** e
  distribui o código sob MIT, mantendo o cabeçalho original intacto. Compilado
  como `unzbm` (executável) e como `libzbitmap.so` (biblioteca, via `ctypes`).

O HkiPadOS não modifica esses arquivos. O que o HkiPadOS acrescenta está em
`hkipados/apfsx.py`, que os importa e relata onde eles param. `scripts/fetch-hakios.sh`
confere os SHA-256 acima contra o commit fixado.
