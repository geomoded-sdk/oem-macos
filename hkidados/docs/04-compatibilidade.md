# 4. Compatibilidade

## 4.1 Como ler os estados

| estado | significa |
|---|---|
| **OK** | feito e verificado neste ambiente, com a evidência citada |
| **PARCIAL** | parte funciona; o que falta está nomeado |
| **BLOQUEADO** | há uma razão concreta, e ela está escrita |
| **NÃO TESTADO** | poderia ser feito, não foi |

Nenhum requisito fica "OK" por ter compilado. A tabela abaixo é gerada por
`./bin/hkidados compat`; `reports/compat.md` é a versão que o código escreve.

## 4.2 O hardware do alvo, medido no DeviceTree

O `DeviceTree.j517ap.im4p` descomprime para 260.176 bytes e declara, entre outros:

| o que | nó/propriedade no DeviceTree | o que o HakiOS usa |
|---|---|---|
| SoC | `arm-io,t8103` | `arm-io` de SM7325/SC7280 |
| interrupções | `aic,1`, `aic-version`, `aic-timebase` | **GICv3** (`-M virt,gic-version=3`; `drivers/interrupt`) |
| IOMMU | `dart,t8020`, `dart-ane`, `dart-aop`, `dart-sep`, `dart-apcie0` | SMMU/DART do SoC Qualcomm |
| armazenamento | `ans`, `nvme-queue-entries`, `nvme-linear-sq`, `sart,coastguard` | UFS (`drivers/storage`) |
| GPU | `gpu,t8103`, `gpu-pwc-win-size`, `gpu-dpe-sync` | Adreno 642L (`drivers/gpu`, `hakios-metal`) |
| display | `dcp`, `display-crossbar,t8103`, `display-boot-rotation` | DSI/DPU do SoC Qualcomm |
| toque | `touch` | controlador do POCO |
| SEP | `sep`, `iop-sep,ascwrap-v4`, `sepfw-load-at-boot`, `effective-production-status-sep` | stubs (`MiniAppleKeyStore`, AMFI mínimo) |
| ANE | `ane,t8020`, `ane-dpe` | não existe |
| ISP | `isp,t8103` | não existe |
| Wi-Fi | `wlan-pcie,bcm4387` | não existe |
| PMU | `pmu,sera`, `pmu,simetra`, `pmu,spmi` | PMIC do SoC Qualcomm |
| relógio/energia | `clpc,t8103`, `pmgr` | `clk`/`rpmh` do SoC Qualcomm |
| volumes | `/private/xarts`, `/private/preboot`, `/private/var` | — |

Cada linha dessa tabela é uma peça de driver que não existe para o alvo. O
DeviceTree é a evidência: não é "achismo" sobre o M1, é o que o firmware do
iPad13,4 declara.

## 4.3 Os quatro bloqueios que decidem

### 1. Assinatura e cadeia de boot

O iPad13,4 só executa `LLB`/`iBoot`/`kernelcache` assinados pela Apple e
personalizados pelo `TSS` para *aquele* aparelho (`Ap,VM`, `RestoreAttestationMode`).
Não há fastboot, não há slot A/B, não há "bootloader desbloqueado". Instalar o
loader do HakiOS exigiria quebrar o secure boot — o que este projeto não faz, por
regra e por ética.

O loader do HakiOS também se apresenta como **iPhone X** (`iPhone10,3/10,6`,
`d22ap`): é essa identidade que o `dyld` e o `launchd` veem. O alvo é `j517ap`,
outro DeviceTree, outra classe.

### 2. SEP

A userland do iPadOS 27 fala com o SEP para chaves, atestação, `MobileKeyBag`,
`AppleKeyStore`, `FairPlay`, `AMFI`. O HakiOS **substitui** o SEP por stubs. Não é
uma diferença de driver: é a ausência do serviço.

### 3. Geração do kernel

| | alvo | HakiOS |
|---|---|---|
| XNU | `13432.42.3` (Darwin 27.2.0) | `8796.121.2` (Darwin 22.5.0) |
| Mach-O | `MH_FILESET`, arm64e, chained fixups | `MH_EXECUTE`, arm64 (`DEVELOPMENT_ARM64_BCM2837`) |
| código aberto mais novo | — | `xnu-12377.121.6` (ainda anterior ao 13432) |

O `launchd` do alvo é arm64e com `ptrauth` e 32 entitlements que pedem
`com.apple.private.kernel.darkboot`, `com.apple.private.pmap.load-trust-cache`,
`com.apple.private.security.storage.launchd` e `com.apple.private.vfs.pivot-root`.
Nenhum desses contratos existe no kernel de 2022.

### 4. AEA

O volume do sistema (8,3 GB) e o `SystemOS` (2,4 GB) são AEA. A chave é entregue
pelo `wkms` a um aparelho autenticado. Sem a chave não há volume, e sem volume não
há `SpringBoard`, `BackBoard` nem frameworks.

## 4.4 O que foi verificado, e como

| verificação | resultado |
|---|---|
| download e SHA-256 do IPSW | confere |
| CRC32 de cada um dos 185 membros | confere |
| digest sha384 do DeviceTree e do kernelcache | confere |
| leitura do BuildManifest por HTTP Range | 185 KB em vez de 11,6 GB |
| kernel: `MH_FILESET`, arm64e, chained fixups | confirmado |
| APFS selado do cryptex de App | leitor próprio: 1.155 arquivos, 5.023 hashes, idêntico ao 7-Zip |
| userland do ramdisk | 387 Mach-O arm64e, `launchd` incluído |
| kernel do HakiOS em QEMU | inicia e executa o processo 1 |

## 4.5 A tabela gerada

<!-- gerado por `make compat`; a fonte é reports/compat.md -->

| Área | Requisito | Estado | Motivo |
|---|---|---|---|
| Boot | Existe um caminho de boot utilizável no alvo | **BLOQUEADO** | o iPad13,4 só executa código assinado pela Apple; instalar outro loader exige quebrar o secure boot, o que este projeto não faz |
| Boot | A identidade de placa que o loader apresenta é a do alvo | **BLOQUEADO** | o loader do HakiOS se apresenta como iPhone X para o dyld e o launchd; o DeviceTree do alvo (DeviceTree.j517ap.im4p) é outro |
| Hardware | GPU: o Metal do HakiOS no hardware do alvo | **BLOQUEADO** | o driver Metal do HakiOS fala com a GPU da Qualcomm; a AGX da Apple tem comando, memória e firmware próprios |
| Hardware | Entrada: toque e botões | **BLOQUEADO** | o controlador de toque da Apple é outro, com firmware assinado |
| Hardware | SEP: serviços de segurança e chaves | **BLOQUEADO** | o HakiOS substitui o SEP por stubs (MiniAppleKeyStore, AMFI mínimo); a userland do iPadOS espera serviços reais do SEP |
| Kernel | O kernel do alvo e o kernel comunitário são a mesma geração | **BLOQUEADO** | a userland de 2026 usa traps e syscalls que o XNU de 2022 não tem; nem o código aberto mais recente (12377) alcança o kernel 13432 do 24B156 |
| Kernel | A arquitetura do binário bate com a do kernel | **BLOQUEADO** | a userland do M1 é arm64e (PAC); o port comunitário compila ARM64 sem arm64e |
| Kernel | Os drivers de plataforma existem para o SoC do alvo | **BLOQUEADO** | GICv3, timer, UART, interrupções e o platform expert do HakiOS são dos SoCs Qualcomm/MediaTek; o M1 não tem Device Tree no formato que o loader lê |
| Sistema de arquivos | Montar o volume do sistema do IPSW | **BLOQUEADO** | AEA com chave entregue pelo wkms da Apple, autenticada pelo dispositivo; decifrar exigiria contornar a proteção |
| Userland | Iniciar o launchd original do sistema | **BLOQUEADO** | o launchd extraído é o do RestoreRamDisk; o do sistema está no volume AEA. Além disso ele é arm64e e exige AMFI/SEP e um kernel da mesma geração |
| Userland | Iniciar BackBoard/SpringBoard e a interface | **BLOQUEADO** | sem o volume do sistema não há o que iniciar; e sem os drivers de tela/touch do M1 não haveria saída |
| Sistema de arquivos | Ler o cryptex de App (não cifrado) | **OK** | `apfs_sealed.py` lê a árvore selada (NOHEADER + HASHED) e a árvore de extents; 1.155 arquivos, 869 fluxos LZBITMAP descomprimidos, byte a byte igual ao 7-Zip |
| Userland | Bibliotecas e frameworks oficiais disponíveis | **PARCIAL** | são componentes reais do iPadOS 27.2.1, mas apenas os que estão fora do AEA |
| Kernel | Compilar o kernel do HakiOS neste ambiente | **NÃO TESTADO** | o build exige a toolchain do theos; o binário pré-compilado já prova o boot |
| Boot | Iniciar o kernel comunitário em QEMU | **OK** | o kernel comunitário inicia e executa userland em QEMU (virt + AAVMF); o que falta não é o kernel, é a userland do iPadOS e o hardware |
| IPSW | Baixar o firmware indicado | **OK** | baixado por HTTP da CDN da Apple e conferido contra o ipsw.me |
| IPSW | Verificar a integridade do arquivo | **OK** | tamanho e SHA-256 conferem com o ipsw.me; o ZIP tem 185 membros |
| IPSW | Ler o BuildManifest e identificar os componentes | **OK** | lido por HTTP Range antes de baixar o IPSW inteiro |
| Kernel | Reutilizar o XNU do HakiOS sem reescrevê-lo | **OK** | patch e arquivos preservados; o HkiPadOS só acrescenta ferramentas |

## 4.6 O que destravaria cada bloqueio

| bloqueio | o que teria de acontecer |
|---|---|
| assinatura | nada legítimo: é a raiz de confiança do aparelho |
| SEP | um dia, engenharia reversa pública do SEP; não existe |
| geração do kernel | a Apple publicar o `xnu-13432` e alguém portá-lo para arm64e + AIC + DART |
| AEA | a Apple publicar a chave, ou o `wkms` aceitar outro aparelho |
| GPU/touch/display | drivers para AGX, DCP e o controlador de toque, com firmware |
| APFS selado | **resolvido**: `apfs_sealed.py` lê a árvore por hash e a de extents |
| `decmpfs` LZBITMAP | **resolvido**: `libzbitmap.c` do HakiOS, compilado e chamado por ctypes |

Os dois primeiros itens dessa tabela eram os únicos que este projeto podia
atacar sozinho, e foram atacados: hoje o cryptex selado é lido e descomprimido
sem o 7-Zip. O que resta depende da Apple (assinatura, SEP, AEA) ou de drivers
para hardware que não existe neste ambiente.
