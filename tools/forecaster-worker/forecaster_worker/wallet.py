"""The worker's own wallet: a 12-word phrase in a file only the owner can read.

Same key scheme as the website wallet (frontend walletCrypto/firstPartyKeys.ts):
BIP39 phrase -> PBKDF2-HMAC-SHA512 seed -> HMAC-SHA512("ed25519 seed", seed)[:32] -> ed25519.
Address = "0x" + first 20 bytes of SHA3-256 of the raw public key.

Nothing in this module prints or logs the phrase or the key.
"""
import hashlib
import hmac
import os
import secrets
import stat
import sys
import unicodedata
from pathlib import Path

from . import ed25519

PHRASE_FILE = "wallet-phrase.txt"
_WORDS_PATH = Path(__file__).with_name("bip39_english.txt")
WORDLIST_SHA256 = "2f5eed53a4727b4bf8880d8f3f199efc90e58503646d9ff8eff3a2ed3b24dbda"  # pragma: allowlist secret -- public checksum of the BIP-39 word list


def wordlist() -> list[str]:
    raw = _WORDS_PATH.read_bytes()
    if hashlib.sha256(raw).hexdigest() != WORDLIST_SHA256:
        raise RuntimeError("the BIP39 word list file is damaged")
    return raw.decode("ascii").split()


def new_phrase(entropy: bytes | None = None) -> str:
    entropy = secrets.token_bytes(16) if entropy is None else entropy
    if len(entropy) != 16:
        raise ValueError("a 12-word phrase needs 16 bytes of entropy")
    words = wordlist()
    bits = (int.from_bytes(entropy, "big") << 4) | (hashlib.sha256(entropy).digest()[0] >> 4)
    return " ".join(words[(bits >> (11 * (11 - i))) & 0x7FF] for i in range(12))


def phrase_is_valid(phrase: str) -> bool:
    parts = phrase.split()
    words = wordlist()
    if len(parts) != 12 or any(w not in words for w in parts):
        return False
    bits = 0
    for w in parts:
        bits = (bits << 11) | words.index(w)
    entropy = (bits >> 4).to_bytes(16, "big")
    return (bits & 0xF) == hashlib.sha256(entropy).digest()[0] >> 4


def address_from_public_key(pub: bytes) -> str:
    return "0x" + hashlib.sha3_256(pub).hexdigest()[:40]


class Wallet:
    """Holds the signing seed in memory. `repr` shows the address only."""

    def __init__(self, phrase: str):
        norm = unicodedata.normalize("NFKD", " ".join(phrase.split()))
        seed = hashlib.pbkdf2_hmac("sha512", norm.encode(), b"mnemonic", 2048, 64)
        self._seed = hmac.new(b"ed25519 seed", seed, hashlib.sha512).digest()[:32]
        self.public_key = ed25519.public_key(self._seed)
        self.address = address_from_public_key(self.public_key)

    def sign(self, message: bytes) -> bytes:
        return ed25519.sign(self._seed, message)

    def __repr__(self) -> str:
        return f"Wallet({self.address})"


def folder_keeps_modes(folder: Path) -> bool:
    """True when a file made private in this folder really is private (false on Windows-backed mounts)."""
    probe = folder / f".mode-probe-{secrets.token_hex(4)}"
    try:
        fd = os.open(probe, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        os.close(fd)
        return stat.S_IMODE(probe.stat().st_mode) & 0o077 == 0
    except OSError:
        return True     # cannot tell: stay strict
    finally:
        try:
            probe.unlink()
        except OSError:
            pass


def load_or_create(data_dir: Path) -> tuple[Wallet, bool]:
    """Return (wallet, created). The phrase file is created with mode 600 and never rewritten."""
    path = Path(data_dir) / PHRASE_FILE
    if path.exists():
        mode = stat.S_IMODE(path.stat().st_mode)
        if mode & 0o077:
            if folder_keeps_modes(Path(data_dir)):
                raise PermissionError(f"{path} is readable by other users (mode {mode:o}); run: chmod 600 {path}")
            # Windows and some network folders ignore Unix file modes. Refusing would lock the owner out.
            print(f"Note: this folder does not keep Unix file permissions, so {PHRASE_FILE} is protected only by "
                  "the folder's own access rules. Keep the folder private.", file=sys.stderr)
        try:
            phrase = path.read_text(encoding="ascii").strip()
        except ValueError:
            phrase = ""
        if not phrase_is_valid(phrase):
            raise ValueError(f"{path} does not hold a valid 12-word phrase. The worker will not replace it. "
                             "Restore your copy of the file, or move it away to start a new wallet.")
        return Wallet(phrase), False
    Path(data_dir).mkdir(parents=True, exist_ok=True)
    phrase = new_phrase()
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w", encoding="ascii") as handle:
        handle.write(phrase + "\n")
    return Wallet(phrase), True
