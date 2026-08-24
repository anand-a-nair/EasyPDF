#!/usr/bin/env python3
"""Regenerates the test corpus in tests/corpus.

The fixtures are hand-built rather than produced by a PDF library: they stay
tiny, they are byte-stable across runs, and every byte is understood. A fixture
nobody can read is a fixture nobody can debug.

Large real-world documents are pinned by hash instead, not committed — see
ideas/04-performance-budget.md.
"""
import pathlib


def build_pdf(text: str, width: int = 200, height: int = 100) -> bytes:
    content = f"BT /F1 24 Tf 20 40 Td ({text}) Tj ET".encode("ascii")

    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        (
            f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] "
            f"/Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>"
        ).encode("ascii"),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
        b"<< /Length " + str(len(content)).encode() + b" >>\nstream\n" + content + b"\nendstream",
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
    out += (
        f"trailer\n<< /Size {len(objects) + 1} /Root 1 0 R >>\n"
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

    for path in sorted(corpus.iterdir()):
        print(f"{path.name:20} {path.stat().st_size:>6} bytes")


if __name__ == "__main__":
    main()
