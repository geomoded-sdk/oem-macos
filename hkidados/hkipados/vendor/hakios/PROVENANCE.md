# Proveniência

Estes dois arquivos são cópias **verbatim** do projeto HakiOS. Não foram editados.

| arquivo | origem | sha256 |
|---|---|---|
| `apfs.py` | `tools/apfs/apfs.py` | `a051520d0c1f4f9d783c484be3beaeb536a1b4364fd8ed2e6a9b5c2d98592588` |
| `dmg.py`  | `tools/apfs/dmg.py`  | `d7aab6e514a3b6a63cc666bd6129529e315e04fb2eff897b6333ce8d2dba8423` |

- Repositório: <https://github.com/oosexgang/hakios-xnu-kernel>
- Commit: `63fc2b4633bac3af8e57dadab9f5a8d4cb940438`
- Licença: MIT, conforme o cabeçalho do repositório (`LICENSE`). O `apfs.py` lê
  estruturas descritas em `xnu/files/bsd/miscfs/apfs/apfs_format.h`, que é APSL 2.0.

O HkiPadOS não modifica esses arquivos. O que o HkiPadOS acrescenta está em
`hkipados/apfsx.py`, que os importa e relata onde eles param. `scripts/fetch-hakios.sh`
confere os SHA-256 acima contra o commit fixado.
