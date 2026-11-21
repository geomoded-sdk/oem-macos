PYTHON ?= python3
IPSW ?= cache/iPad_Pro_Spring_2021_27.2.1_24B156_Restore.ipsw
IMAGE ?=
JSON ?= reports/ipad13,4-24B156.json

.PHONY: help doctor test analyze userland deps apfs unzbm compat boot-open init-attempts check clean

help:
	@sed -n 's/^#  /  /p' Makefile

doctor:
	./bin/hkidados doctor

#  tudo o que não precisa do IPSW
test:
	$(PYTHON) -m unittest discover -s tests -v

check: test
	$(PYTHON) -m compileall -q hkipados

analyze:
	./bin/hkidados analyze $(IPSW) --json $(JSON) --quiet

userland: unzbm
	./bin/hkidados userland $(IPSW) --decompress --unzbm build/bin/unzbm \
		--json reports/userland.json

#  as dependências dos binários extraídos
deps:
	./bin/hkidados deps build/userland/restore-ramdisk --json reports/deps.json
	./bin/hkidados deps build/userland/app-cryptex --json reports/deps-cryptex.json

#  um volume APFS (cru, IM4P ou UDIF) lido e descomprimido sem o 7-Zip
apfs:
	@test -n "$(IMAGE)" || { echo "uso: make apfs IMAGE=imagem.dmg"; exit 2; }
	./bin/hkidados apfs $(IMAGE) --out build/apfs --decompress

#  o decodificador LZBITMAP do HakiOS, compilado do fonte
unzbm:
	$(PYTHON) -c 'from hkipados import decmpfs; p, why = decmpfs.build_unzbm("build/bin/unzbm"); print(p or why); raise SystemExit(1 if p is None else 0)'

compat:
	./bin/hkidados compat $(IPSW) --userland build/userland \
		--json reports/compat.json --md reports/compat.md

#  o kernel aberto do HakiOS no QEMU (não usa o IPSW)
boot-open:
	scripts/hakios-boot-open.sh

#  o init do iPadOS nesse kernel: 4 casos, um boot cada (precisa de `make userland`)
init-attempts:
	scripts/try-ipados-init.sh

clean:
	rm -rf build reports/*.raw
	find . -name __pycache__ -type d -prune -exec rm -rf {} +
