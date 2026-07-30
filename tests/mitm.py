"""A malicious relay endpoint: speaks the presse pairing protocol in Python,
so tests can man-in-the-middle the two devices. Mirrors docs/protocol.md."""

import hashlib
import hmac as hmac_mod

from ecdsa import SigningKey, VerifyingKey, SECP256k1

COMMIT_TAG = b"presse-commit"
SAS_TAG = b"presse-sas"
SESSION_TAG = b"presse-session"
BEARER_TAG = b"presse-bearer"


class MitmEndpoint:
    """One side of the MITM: a fake device with its own ephemeral."""

    def __init__(self):
        self.sk = SigningKey.generate(curve=SECP256k1)
        self.pub = self.sk.get_verifying_key().to_string("uncompressed")
        self.session_key = None
        self.sas = None
        self.send_seq = 0
        self.recv_seq = 0

    def commitment(self) -> bytes:
        return hashlib.sha256(COMMIT_TAG + self.pub).digest()

    def derive(self, peer_pub: bytes, as_master: bool):
        peer = VerifyingKey.from_string(peer_pub, curve=SECP256k1)
        point = self.sk.privkey.secret_multiplier * peer.pubkey.point
        secret = int(point.x()).to_bytes(32, "big")
        if as_master:
            master_pub, receiver_pub = self.pub, peer_pub
        else:
            master_pub, receiver_pub = peer_pub, self.pub
        transcript = hashlib.sha256(SAS_TAG + master_pub + receiver_pub).digest()
        self.session_key = hmac_mod.new(secret, SESSION_TAG + transcript, hashlib.sha256).digest()
        self.sas = hmac_mod.new(secret, SAS_TAG + transcript, hashlib.sha256).digest()[:4]

    def bearer_xor(self, ins: int, seq: int, key: bytes) -> bytes:
        """Seal or open a 32-byte bearer key: XOR against a session-keyed pad.
        A MITM that fooled both humans holds the session key on each side and
        can therefore read the key in flight, which is precisely the property
        the SAS step exists to prevent and these tests exist to pin."""
        pad = hmac_mod.new(
            self.session_key, BEARER_TAG + bytes([ins, seq]), hashlib.sha256
        ).digest()
        return bytes(a ^ b for a, b in zip(key, pad))

    def mac_send(self, ins: int, payload: bytes) -> bytes:
        mac = hmac_mod.new(
            self.session_key, bytes([ins, self.send_seq]) + payload, hashlib.sha256
        ).digest()
        self.send_seq += 1
        return mac

    def mac_verify(self, ins: int, payload: bytes, mac: bytes) -> bool:
        expected = hmac_mod.new(
            self.session_key, bytes([ins, self.recv_seq]) + payload, hashlib.sha256
        ).digest()
        self.recv_seq += 1
        return hmac_mod.compare_digest(expected, mac)
