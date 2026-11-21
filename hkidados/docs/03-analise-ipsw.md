# 3. Análise do IPSW do iPad13,4

Tudo aqui veio de `./bin/hkidados analyze` e está em `reports/ipad13,4-24B156.json`.

## 3.1 O arquivo

| | |
|---|---|
| tamanho | 11.655.546.609 bytes |
| SHA-256 | `b00bb0cbc17a810ebd36bb4befa6cc7599c1abdbf92093abf2a4db2d8aebe333` |
| membros | 185, ZIP64, 11.828.110.330 bytes descomprimidos |
| manifesto | 185.172 bytes comprimidos, 3.092.175 descomprimidos |
| versão | iPadOS 27.2.1, build 24B156 |
| produtos | iPad13,4 até iPad13,11 (o mesmo IPSW serve os oito) |

## 3.2 Identidades de restauração

O `BuildManifest` traz **24 identidades**: oito classes de aparelho
(`j517ap`, `j517xap`, `j518ap`, `j518xap`, `j522ap`, `j522xap`, `j523ap`,
`j523xap`) vezes três variantes (`Customer Erase Install (IPSW)`,
`Customer Upgrade Install (IPSW)`, `Recovery Customer Install`). O iPad13,4 é
`j517ap`. As identidades que interessam ao boot, na variante de instalação:

| identidade | arquivo | tamanho |
|---|---|---|
| `iBSS` | `Firmware/dfu/iBSS.j517.RELEASE.im4p` | 1.126.220 |
| `iBEC` | `Firmware/dfu/iBEC.j517.RELEASE.im4p` | 1.126.229 |
| `LLB` | `Firmware/all_flash/LLB.j517.RELEASE.im4p` | 1.126.220 |
| `iBoot` | `Firmware/all_flash/iBoot.j517.RELEASE.im4p` | 1.126.220 |
| `DeviceTree` | `Firmware/all_flash/DeviceTree.j517ap.im4p` | 47.255 |
| `SEP` / `RestoreSEP` | `Firmware/all_flash/sep-firmware.j517.RELEASE.im4p` | 5.949.720 |
| `KernelCache` | `kernelcache.release.ipad13` | 23.257.193 |
| `StaticTrustCache` | `Firmware/141-10293-225.dmg.aea.trustcache` | 86.209 |
| `RestoreRamDisk` | `141-10399-248.dmg` | 245.366.811 |

`iBSS`, `LLB` e `iBoot` terem o mesmo tamanho não é erro de leitura: nesses
aparelhos o iBSS é o próprio iBoot, carregado no modo DFU. O `iBEC` difere por
9 bytes.

Cada identidade traz `Digest` (sha1), `Trusted`/`Restore` e as variantes de
`Info`/`Manifest`. O HkiPadOS confere o `Digest` do DeviceTree e do kernelcache
contra o arquivo real: **sha384, ambos conferem** (`manifest_digest_ok: true`).

## 3.3 O kernel

```
KernelManagement_host-514.40.8
Darwin Kernel Version 27.2.0: Tue Oct 13 21:25:46 PDT 2026;
root:xnu-13432.42.3~28/RELEASE_ARM64_T8103
```

| | |
|---|---|
| IM4P | tipo `krnl`, 23.257.193 bytes de payload, LZFSE |
| descomprimido | 76.775.424 bytes |
| Mach-O | `MH_FILESET`, `arm64e` (`cpusubtype 0xc0000002`), plataforma `iOS` |
| chained fixups | sim |
| xnu | `13432.42.3`, build `~28`, `RELEASE_ARM64_T8103` |

O mesmo `kernelcache.release.ipad13` aparece duas vezes no manifesto, como
`KernelCache` e `RestoreKernelCache`. O HkiPadOS lê o IM4P **uma vez** e preenche
os dois.

`MH_FILESET` + `arm64e` + chained fixups é o que torna o kernel de 2026 diferente
do que o HakiOS compila (`DEVELOPMENT_ARM64_BCM2837`, arm64).

## 3.4 As imagens

| componente | container | tamanho | chave |
|---|---|---|---|
| `OS` | AEA | 8.321.499.136 | `https://wkms.sd.apple.com` |
| `Cryptex1,SystemOS` | AEA | 2.415.919.104 | `https://wkms.sd.apple.com` |
| `Cryptex1,AppOS` | APFS | 16.777.216 | — |
| `RestoreRamDisk` | APFS (offset 27) | 245.366.811 | — |

### AEA

O cabeçalho do `141-10293-225.dmg.aea`, lido pelo `hkipados/aea.py`:

```
AEA1, versão 1, header_size 2240, 0 campos indecifráveis
com.apple.wkms.url        = https://wkms.sd.apple.com
com.apple.wkms.auth-data  = (base64, ~600 bytes: atestação do aparelho)
com.apple.wkms.fcs-response = {"enc-request": "...", "wrapped-key": "..."}
com.apple.wkms.fcs-key-url = https://wkms-public.apple.com/fcs-keys/11tfUMoA...=
```

AEA é "Apple Encrypted Archive". A chave não está no arquivo: o `wrapped-key` é
desembrulhado pelo `wkms` (Key Management Service) da Apple para um aparelho
autenticado, e o `auth-data` é a atestação desse aparelho. O `hkipados/aea.py` lê
o cabeçalho e para aí — decifrar exigiria a chave, e obtê-la seria contornar a
proteção. O relatório de compatibilidade trata isso como `BLOQUEADO`, com essa
razão.

### APFS

O cryptex de App é APFS **selado**: superbloco `NXSB` em offset 0, bloco de
4096 bytes, 4096 blocos (16 MiB). O RestoreRamDisk é APFS com um prefixo de 27
bytes antes do `NXSB`, 4096 bytes por bloco, 59.904 blocos (245.366.784 bytes).

O leitor do HakiOS (`hkipados/vendor/hakios/apfs.py`) verifica Fletcher-64 em todo
bloco e ordena a árvore por object-id. A árvore do volume selado não tem checksum
no nó (`BTNODE_NOHEADER`) e ordena por hash; por isso ele não abre o volume. O
7-Zip 23+ abre. O `apfsx.py` detecta o caso e usa o 7-Zip, dizendo que usou.

## 3.5 O que mais está no IPSW

- `Ap,SystemVolumeCanonicalMetadata` — 37.525.144 bytes de `.mtree` do volume do
  sistema (o inventário de arquivos que o sistema selado teria).
- `SE,UpdatePayload` — firmware do SEP (`Stockholm7.RELEASE.sefw`, 21.641.968).
- `ISP` — firmware da câmera (`adc-pallas-j5x.im4p`, 20.119.579).
- `Baseband`, `PSECipher`, `RestoreTrustCache`, `StaticTrustCache`, e o
  `iBoot`/`LLB`/`iBSS`/`iBEC`/`DeviceTree` acima.

## 3.6 O DeviceTree

`DeviceTree.j517ap.im4p` tem 47.255 bytes e é um IM4P no formato **clássico**
(sem o embrulho DER de 2020 em diante):

```
tipo "dtre", descrição "EmbeddedDeviceTrees-12661.42.3"
payload 47.193 bytes, magic "bvx2" → LZFSE, 10 bytes de trailer
SHA-384 confere com o manifesto
```

É o arquivo que descreve o hardware do iPad para o kernel — e é exatamente o que
o loader do HakiOS **não** usa: ele monta um Device Tree próprio, com a
identidade de um iPhone X.
