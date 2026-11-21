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

`scripts/fetch-hakios.sh` confere esses hashes. Nada neles foi alterado; as
modificações do HkiPadOS vivem em `hkipados/apfsx.py`, que os importa.

## Apple

Nenhum arquivo da Apple é distribuído neste repositório. O IPSW é obtido pelo
usuário, da Apple, e fica em `cache/` (fora do controle de versão). "iPadOS",
"Darwin", "XNU", "APFS", "AEA" e "Apple" são citados para descrever com o que o
código trabalha. Este projeto não é afiliado nem endossado pela Apple.

## Ferramentas de terceiros

- **7-Zip** (Igor Pavlov, LGPL + unRAR restriction) — usado como processo externo
  para ler APFS selado. Não é distribuído aqui; `apt install 7zip`.
- **lzfse** (pacote Python) — descompressão LZFSE do kernelcache. Opcional.
- **QEMU** e **AAVMF/edk2** — usados no teste de boot aberto. Não distribuídos.

## O que o HkiPadOS acrescenta

Tudo em `hkipados/` (exceto `hkipados/vendor/`), `bin/`, `scripts/`, `tests/` e
`docs/` é do HkiPadOS, sob MIT (`LICENSE`).
