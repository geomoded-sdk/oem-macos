# 7. Testes: o que rodou, o que deu, o que quebrou

Nada aqui é "deve funcionar". Cada linha tem o comando e o resultado. Os logs
completos: `reports/tests.log`, `reports/userland.log`, `reports/qemu-open.log`.

## 7.1 Resultado geral

```
$ HKIPADOS_IPSW=cache/iPad_Pro_Spring_2021_27.2.1_24B156_Restore.ipsw \
      python3 -m unittest discover -s tests -v
Ran 25 tests in 3.084s
OK (skipped=1)
```

O único pulado é o que lê o manifesto por HTTP (`HKIPADOS_URL`), para a suíte
rodar sem rede.

| etapa | comando | resultado |
|---|---|---|
| integridade do IPSW | `sha256sum`, `ZipArchive` | 11.655.546.609 bytes, SHA-256 confere, 185 CRCs conferem |
| manifesto | `hkidados analyze` | 24 identidades, 84 componentes, digest sha384 de DeviceTree e kernelcache confere |
| kernel | idem | `MH_FILESET`, arm64e, chained fixups, 76.775.424 bytes, xnu-13432.42.3 |
| AEA | idem | versão 1, header 2240, `wkms.sd.apple.com`, 0 campos indecifráveis |
| APFS | idem | App: offset 0, 4096/4096; RamDisk: offset 27, 4096/59.904 |
| extração | `hkidados userland` | 2.893 + 1.564 entradas, 9,1 MB + 2,0 GB |
| userland | idem | 383 Mach-O arm64e; `launchd` iOS 27.2.0 com 32 entitlements |
| decmpfs | idem | 869 fluxos, todos LZBITMAP; 867 com `ZBM`; 17 com Mach-O universal |
| compatibilidade | `hkidados compat` | 11 BLOQUEADO, 2 PARCIAL, 5 OK, 1 NÃO TESTADO |
| kernel em QEMU | `scripts/hakios-boot-open.sh` | `bsd_init: done`, processo 1 executou |
| testes unitários | `make test` | 25 testes, 24 ok, 1 pulado |

## 7.2 O boot do kernel comunitário, em detalhe

```
$ QEMU_MEM=3G QEMU_SMP=2 qemu/run.sh 220
```

| instante | o que aconteceu |
|---|---|
| primeiros segundos | AAVMF inicia `\EFI\BOOT\BOOTAA64.EFI` |
| — | `== XNULOADER v3 ==`, banner, `kernel.ver=2.3.4-hakios-darwin22.5-g63fc2b46` |
| — | payload entregue: kernel 16,8 MB, ramdisk 4 KB, cmdline `cpus=1` |
| ~5,9 s | `Darwin Kernel Version 22.5.0 … xnu-8796.121.2 … DEVELOPMENT_ARM64_BCM2837` |
| 7,2 s | AMFI e Sandbox "stand-in" carregados; `GenericARMCPU: cpu 0 registered` |
| 7,49 s | `BSD root: md0, major 1, minor 0` |
| 7,51 s | `bsd_init: done` |
| 7,53 s | `load_init_program: attempting to load /sbin/launchd` |
| 7,63 s | processo 1 imprime `Hello from user space on Darwin!` e sai |

O processo 1 é o `hello` do HakiOS, o init da configuração aberta; não é o
`launchd` da Apple. O kernel o executa, ele roda e sai — que é o que se queria
provar.

## 7.3 O que quebrou no caminho, e o conserto

| problema | sintoma | conserto |
|---|---|---|
| IM4P "clássico" (DeviceTree) | parser lia o tipo como tag DER e falhava | aceitar tipo cru de 4 bytes sem tag IA5 |
| CRC de teste | teste comparava CRC errado | CRC calculado sobre os bytes reais |
| APFS com prefixo | RestoreRamDisk dava `unknown` | procurar `NXSB` nos primeiros 4096 bytes e reportar `apfs_offset` |
| AEA | campos `wkms.*` com nome estranho | ler o campo como par comprimento/valor e reportar `undecodable` |
| `find_fat` | não achava o cabeçalho no último offset possível | limite `len-7`, não `len-8` |
| 7-Zip e APFS selado | leitor do HakiOS para no nó selado | usar 7-Zip só nesse caso, e dizer que usou |
| arquivos com 0 byte no cryptex | 7-Zip não remonta `decmpfs` | detectar e documentar; não fingir que extraiu |

## 7.4 O que não foi testado

| o que | por quê |
|---|---|
| boot em hardware Apple | exigiria quebrar a assinatura; não é feito |
| boot da userland do iPadOS | volume do sistema em AEA |
| `SpringBoard`/`BackBoard` | não estão em nenhuma imagem legível |
| recompilar o kernel | falta a toolchain do theos; usei `prebuilt/` |
| montar o APFS selado com o leitor do HakiOS | ele não suporta a árvore por hash |
| descomprimir `decmpfs` tipo 14 | não há decodificador LZBITMAP no 7-Zip nem no HakiOS |

## 7.5 Como repetir

```sh
make test                     # 25 testes, sem IPSW
make doctor                   # o que falta nesta máquina
make analyze                  # usa cache/*.ipsw
make userland                 # extrai e inventaria
make compat                   # gera a tabela
make boot-open                # kernel do HakiOS no QEMU (precisa de qemu + AAVMF)
```

O `make userland` apaga as imagens intermediárias no fim; com `--keep` elas
ficam em `build/userland/`.
