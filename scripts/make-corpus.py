#!/usr/bin/env python3
"""Regenerates the test corpus in tests/corpus.

The fixtures are hand-built rather than produced by a PDF library: they stay
tiny, they are byte-stable across runs, and every byte is understood. A fixture
nobody can read is a fixture nobody can debug.

Large real-world documents are pinned by hash instead, not committed — see
ideas/04-performance-budget.md.
"""
import hashlib
import pathlib
import struct


def build_pdf(text: str, width: int = 200, height: int = 100) -> bytes:
    return build_multipage([text], width, height)


def build_multipage(texts: list[str], width: int = 200, height: int = 100) -> bytes:
    """Builds a document with one page per entry in `texts`.

    Object numbering: 1 = catalog, 2 = page tree, 3 = font, then a page object
    and a content stream per page.
    """
    page_count = len(texts)
    first_page_obj = 4
    kids = " ".join(f"{first_page_obj + i * 2} 0 R" for i in range(page_count))

    objects: list[bytes] = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        f"<< /Type /Pages /Kids [{kids}] /Count {page_count} >>".encode("ascii"),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    ]

    for index, text in enumerate(texts):
        content = f"BT /F1 24 Tf 20 40 Td ({text}) Tj ET".encode("ascii")
        contents_obj = first_page_obj + index * 2 + 1
        objects.append(
            (
                f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] "
                f"/Resources << /Font << /F1 3 0 R >> >> /Contents {contents_obj} 0 R >>"
            ).encode("ascii")
        )
        objects.append(
            b"<< /Length " + str(len(content)).encode() + b" >>\nstream\n"
            + content + b"\nendstream"
        )

    out = bytearray(b"%PDF-1.7\n")
    offsets = []
    for number, body in enumerate(objects, start=1):
        offsets.append(len(out))
        out += f"{number} 0 obj\n".encode("ascii") + body + b"\nendobj\n"

    xref_at = len(out)
    out += f"xref\n0 {len(objects) + 1}\n".encode("ascii")
    out += b"0000000000 65535 f \n"
    for offset in offsets:
        out += f"{offset:010d} 00000 n \n".encode("ascii")
    out += (
        f"trailer\n<< /Size {len(objects) + 1} /Root 1 0 R >>\n"
        f"startxref\n{xref_at}\n%%EOF\n"
    ).encode("ascii")
    return bytes(out)


# --- encryption -------------------------------------------------------------
#
# PDF's standard security handler, revision 2 (RC4, 40-bit). Implemented here
# rather than pulled from a library so the corpus generator stays
# dependency-free, matching the rest of these hand-built fixtures.
#
# RC4 is long broken, which is exactly why it makes a good *read* fixture:
# EasyPDF must open legacy encrypted documents while refusing to create them.
# See easypdf-crypto's Algorithm::can_write.

PAD = bytes([
    0x28, 0xBF, 0x4E, 0x5E, 0x4E, 0x75, 0x8A, 0x41, 0x64, 0x00, 0x4E, 0x56,
    0xFF, 0xFA, 0x01, 0x08, 0x2E, 0x2E, 0x00, 0xB6, 0xD0, 0x68, 0x3E, 0x80,
    0x2F, 0x0C, 0xA9, 0xFE, 0x64, 0x53, 0x69, 0x7A,
])


def rc4(key: bytes, data: bytes) -> bytes:
    state = list(range(256))
    j = 0
    for i in range(256):
        j = (j + state[i] + key[i % len(key)]) % 256
        state[i], state[j] = state[j], state[i]

    out = bytearray()
    i = j = 0
    for byte in data:
        i = (i + 1) % 256
        j = (j + state[i]) % 256
        state[i], state[j] = state[j], state[i]
        out.append(byte ^ state[(state[i] + state[j]) % 256])
    return bytes(out)


def pad_password(password: str) -> bytes:
    raw = password.encode("latin-1")[:32]
    return raw + PAD[: 32 - len(raw)]


def build_encrypted_pdf(text: str, user_password: str, doc_id: bytes) -> bytes:
    """Builds a single-page RC4-40 encrypted document."""
    permissions = -1  # everything allowed; the flags are advisory anyway
    key_length = 5    # 40 bits

    owner_entry = rc4(
        hashlib.md5(pad_password(user_password)).digest()[:key_length],
        pad_password(user_password),
    )

    key_input = (
        pad_password(user_password)
        + owner_entry
        + struct.pack("<i", permissions)
        + doc_id
    )
    encryption_key = hashlib.md5(key_input).digest()[:key_length]
    user_entry = rc4(encryption_key, PAD)

    def object_key(number: int, generation: int = 0) -> bytes:
        extended = (
            encryption_key
            + struct.pack("<I", number)[:3]
            + struct.pack("<I", generation)[:2]
        )
        return hashlib.md5(extended).digest()[: min(len(encryption_key) + 5, 16)]

    content = f"BT /F1 24 Tf 20 40 Td ({text}) Tj ET".encode("ascii")
    encrypted_content = rc4(object_key(5), content)

    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        (
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] "
            "/Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>"
        ).encode("ascii"),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
        b"<< /Length " + str(len(encrypted_content)).encode() + b" >>\nstream\n"
        + encrypted_content + b"\nendstream",
        (
            b"<< /Filter /Standard /V 1 /R 2 /Length 40 /P " + str(permissions).encode()
            + b" /O <" + owner_entry.hex().encode() + b">"
            + b" /U <" + user_entry.hex().encode() + b"> >>"
        ),
    ]

    out = bytearray(b"%PDF-1.7\n")
    offsets = []
    for number, body in enumerate(objects, start=1):
        offsets.append(len(out))
        out += f"{number} 0 obj\n".encode("ascii") + body + b"\nendobj\n"

    xref_at = len(out)
    out += f"xref\n0 {len(objects) + 1}\n".encode("ascii")
    out += b"0000000000 65535 f \n"
    for offset in offsets:
        out += f"{offset:010d} 00000 n \n".encode("ascii")

    identifier = doc_id.hex()
    out += (
        f"trailer\n<< /Size {len(objects) + 1} /Root 1 0 R /Encrypt 6 0 R "
        f"/ID [<{identifier}> <{identifier}>] >>\n"
        f"startxref\n{xref_at}\n%%EOF\n"
    ).encode("ascii")
    return bytes(out)


def main() -> None:
    corpus = pathlib.Path(__file__).parent.parent / "tests" / "corpus"
    corpus.mkdir(parents=True, exist_ok=True)

    # The happy path.
    (corpus / "minimal.pdf").write_bytes(build_pdf("Hello EasyPDF"))
    # A different aspect ratio, to catch dimension handling that only works
    # for one shape.
    (corpus / "wide.pdf").write_bytes(build_pdf("Wide", width=400, height=100))
    # Not a PDF at all: must be rejected, never guessed at.
    (corpus / "not-a-pdf.bin").write_bytes(
        b"\x00\x01\x02 this is definitely not a PDF \xff\xfe" * 4
    )
    # Valid header, ruined body — the "looks plausible until you read it" case.
    full = build_pdf("Truncated")
    (corpus / "truncated.pdf").write_bytes(full[: len(full) // 2])
    # Enough pages to make per-request re-parsing visible in a benchmark and to
    # exercise scroll virtualisation. Every page carries distinct text so page
    # identity is checkable.
    (corpus / "many-pages.pdf").write_bytes(
        build_multipage([f"Page {n + 1} of 200" for n in range(200)])
    )
    # Encrypted with the user password "secret". Byte-stable because the
    # document ID is fixed rather than random.
    (corpus / "encrypted.pdf").write_bytes(
        build_encrypted_pdf("Secret EasyPDF", "secret", bytes(range(16)))
    )

    for path in sorted(corpus.iterdir()):
        print(f"{path.name:20} {path.stat().st_size:>6} bytes")


if __name__ == "__main__":
    main()
