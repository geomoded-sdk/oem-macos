"""BuildManifest.plist: o índice de tudo o que o IPSW contém.

Cada identidade de build é um (produto, variante). O iPad13,4 aparece em três:
`Customer Erase Install (IPSW)`, `Customer Upgrade Install (IPSW)` e
`Recovery Customer Install`. Cada componente traz um digest SHA-1 de 20 bytes,
o caminho dentro do ZIP e as regras de personalização do TSS.
"""

from __future__ import annotations

import hashlib
import plistlib
from dataclasses import dataclass, field

DIGEST_ALGORITHMS = {20: "sha1", 32: "sha256", 48: "sha384"}


@dataclass
class Component:
    name: str
    path: str | None
    digest: bytes
    info: dict = field(default_factory=dict)

    @property
    def digest_hex(self) -> str:
        return self.digest.hex()

    @property
    def algorithm(self) -> str | None:
        return DIGEST_ALGORITHMS.get(len(self.digest))

    def verify(self, data: bytes) -> bool:
        algorithm = self.algorithm
        if algorithm is None:
            return False
        return hashlib.new(algorithm, data).digest() == self.digest

    @property
    def personalized(self) -> bool:
        return bool(self.info.get("Personalize"))


@dataclass
class Identity:
    product_type: str
    device_class: str
    variant: str
    restore_behavior: str
    components: dict[str, Component]
    raw: dict

    def component(self, name: str) -> Component | None:
        return self.components.get(name)

    def path_of(self, name: str) -> str | None:
        component = self.components.get(name)
        return component.path if component else None

    @property
    def variant_contents(self) -> dict:
        return self.raw.get("Info", {}).get("VariantContents", {})


@dataclass
class BuildManifest:
    product_version: str
    product_build: str
    supported_products: list[str]
    identities: list[Identity]

    @classmethod
    def load(cls, data: bytes) -> "BuildManifest":
        raw = plistlib.loads(data)
        identities = []
        for entry in raw.get("BuildIdentities", []):
            info = entry.get("Info", {})
            components = {}
            for name, value in entry.get("Manifest", {}).items():
                component_info = value.get("Info", {})
                components[name] = Component(
                    name=name,
                    path=component_info.get("Path"),
                    digest=value.get("Digest", b"") or b"",
                    info=component_info,
                )
            identities.append(Identity(
                product_type=entry.get("Ap,ProductType") or info.get("ProductType") or "",
                device_class=info.get("DeviceClass", ""),
                variant=info.get("Variant", ""),
                restore_behavior=info.get("RestoreBehavior", ""),
                components=components,
                raw=entry,
            ))
        return cls(
            product_version=raw.get("ProductVersion", ""),
            product_build=raw.get("ProductBuildVersion", ""),
            supported_products=raw.get("SupportedProductTypes", []),
            identities=identities,
        )

    def identity(self, product_type: str, variant: str | None = None) -> Identity | None:
        candidates = [i for i in self.identities if i.product_type == product_type]
        if variant:
            for identity in candidates:
                if variant.lower() in identity.variant.lower():
                    return identity
        return candidates[0] if candidates else None

    def variants(self, product_type: str) -> list[str]:
        return [i.variant for i in self.identities if i.product_type == product_type]
