#!/usr/bin/env python3
"""Renew the public test fixture certificates. Never reads production credentials."""
from datetime import datetime, timezone, timedelta
from pathlib import Path
from cryptography import x509
from cryptography.x509.oid import NameOID, ExtendedKeyUsageOID
from cryptography.hazmat.primitives import hashes, serialization

directory = Path(__file__).resolve().parents[1] / "GooseRemoteTests" / "Fixtures"
key = serialization.load_pem_private_key((directory / "disposable-key.pem").read_bytes(), password=None)
name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "Goose Remote disposable TLS test")])
for mode in ["valid", "expired", "future", "client-only", "wrong-owner", "renewed"]:
    before = datetime.now(timezone.utc) - timedelta(minutes=5)
    if mode == "expired":
        before = datetime(2020, 1, 1, tzinfo=timezone.utc)
    if mode == "future":
        before = datetime(2040, 1, 1, tzinfo=timezone.utc)
    owner = "22222222-2222-4222-8222-222222222222" if mode == "wrong-owner" else "11111111-1111-4111-8111-111111111111"
    cert = (x509.CertificateBuilder().subject_name(name).issuer_name(name).public_key(key.public_key())
        .serial_number(x509.random_serial_number()).not_valid_before(before).not_valid_after(before + timedelta(days=90))
        .add_extension(x509.SubjectAlternativeName([x509.DNSName("owner-" + owner + ".link")]), False)
        .add_extension(x509.BasicConstraints(ca=False, path_length=None), True)
        .add_extension(x509.KeyUsage(digital_signature=True, content_commitment=False, key_encipherment=False,
            data_encipherment=False, key_agreement=False, key_cert_sign=False, crl_sign=False,
            encipher_only=None, decipher_only=None), True)
        .add_extension(x509.ExtendedKeyUsage([ExtendedKeyUsageOID.CLIENT_AUTH if mode == "client-only" else ExtendedKeyUsageOID.SERVER_AUTH]), False)
        .sign(key, hashes.SHA256()))
    (directory / (mode + ".pem")).write_bytes(cert.public_bytes(serialization.Encoding.PEM))
    (directory / (mode + ".der")).write_bytes(cert.public_bytes(serialization.Encoding.DER))
print("Renewed disposable 90-day test certificates; SPKI key unchanged.")
