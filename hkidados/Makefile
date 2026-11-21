PYTHON ?= python3
IPSW ?= cache/iPad_Pro_Spring_2021_27.2.1_24B156_Restore.ipsw
JSON ?= reports/ipad13,4-24B156.json

.PHONY: help doctor test analyze userland compat boot-open check clean

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

userland:
	./bin/hkidados userland $(IPSW) --json reports/userland.json

compat:
	./bin/hkidados compat $(IPSW) --userland build/userland \
		--json reports/compat.json --md reports/compat.md

#  o kernel aberto do HakiOS no QEMU (não usa o IPSW)
boot-open:
	scripts/hakios-boot-open.sh

clean:
	rm -rf build reports/*.raw
	find . -name __pycache__ -type d -prune -exec rm -rf {} +
