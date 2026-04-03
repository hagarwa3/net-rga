#!/usr/bin/env python3

from __future__ import annotations

from io import BytesIO
from pathlib import Path
from zipfile import ZIP_DEFLATED, ZipFile


ROOT = Path(__file__).resolve().parent
TARGET = ROOT / "data" / "tier1" / "local_fs" / "mixed_small"


def build_pdf(pages: list[str]) -> bytes:
    objects: list[tuple[int, str]] = []
    page_ids: list[str] = []
    font_id = 3 + (len(pages) * 2)

    objects.append((1, "<< /Type /Catalog /Pages 2 0 R >>"))
    for index, page_text in enumerate(pages):
        page_id = 3 + (index * 2)
        content_id = page_id + 1
        page_ids.append(f"{page_id} 0 R")
        objects.append(
            (
                page_id,
                f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents {content_id} 0 R /Resources << /Font << /F1 {font_id} 0 R >> >> >>",
            )
        )
        stream = f"BT\n/F1 18 Tf\n72 720 Td\n({escape_pdf_text(page_text)}) Tj\nET"
        objects.append((content_id, f"<< /Length {len(stream)} >>\nstream\n{stream}\nendstream"))

    objects.insert(1, (2, f"<< /Type /Pages /Kids [{' '.join(page_ids)}] /Count {len(pages)} >>"))
    objects.append((font_id, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"))

    buffer = bytearray()
    buffer.extend(b"%PDF-1.4\n")
    offsets = [0]
    for object_id, body in objects:
        offsets.append(len(buffer))
        buffer.extend(f"{object_id} 0 obj\n{body}\nendobj\n".encode("utf-8"))

    xref_offset = len(buffer)
    buffer.extend(f"xref\n0 {len(objects) + 1}\n".encode("utf-8"))
    buffer.extend(b"0000000000 65535 f \n")
    for offset in offsets[1:]:
        buffer.extend(f"{offset:010} 00000 n \n".encode("utf-8"))
    buffer.extend(
        (
            f"trailer\n<< /Size {len(objects) + 1} /Root 1 0 R >>\n"
            f"startxref\n{xref_offset}\n%%EOF\n"
        ).encode("utf-8")
    )
    return bytes(buffer)


def escape_pdf_text(value: str) -> str:
    return value.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)")


def build_docx(paragraphs: list[str]) -> bytes:
    xml_paragraphs = "\n".join(
        f"<w:p><w:r><w:t>{escape_xml(paragraph)}</w:t></w:r></w:p>" for paragraph in paragraphs
    )
    document_xml = f"""<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    {xml_paragraphs}
  </w:body>
</w:document>
"""
    buffer = BytesIO()
    with ZipFile(buffer, "w", compression=ZIP_DEFLATED) as archive:
        archive.writestr("[Content_Types].xml", CONTENT_TYPES_XML)
        archive.writestr("_rels/.rels", ROOT_RELS_XML)
        archive.writestr("word/_rels/document.xml.rels", WORD_RELS_XML)
        archive.writestr("word/document.xml", document_xml)
    return buffer.getvalue()


def escape_xml(value: str) -> str:
    return (
        value.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
    )


CONTENT_TYPES_XML = """<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>
"""


ROOT_RELS_XML = """<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>
"""


WORD_RELS_XML = """<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>
"""


def write_bytes(relative_path: str, payload: bytes) -> None:
    destination = TARGET / relative_path
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_bytes(payload)


def write_text(relative_path: str, content: str) -> None:
    destination = TARGET / relative_path
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(content, encoding="utf-8")


def main() -> int:
    write_text(
        "README.md",
        """# Tier 1 Mixed Small Corpus

This mixed-format benchmark corpus is generated locally and kept out of git.

It currently includes:

- a simple multi-page PDF fixture for page-anchor evaluation
- a simple DOCX fixture for paragraph/chunk-anchor evaluation
""",
    )
    write_bytes(
        "docs/policies/compliance-policy.pdf",
        build_pdf(
            [
                "Compliance manual overview for Riverglass operations.",
                "General policy guidance and regional controls.",
                "Appendix A and archival handling notes.",
                "Riverglass compliance exception approved for archival systems.",
            ]
        ),
    )
    write_bytes(
        "docs/playbooks/orchid-launch.docx",
        build_docx(
            [
                "Orchid launch planning document.",
                "Regional staffing remains provisional until signoff.",
                "The orchid launch checklist must complete before regional signoff.",
                "Appendix and follow-up notes.",
            ]
        ),
    )
    print(f"materialized Tier 1 mixed-small corpus at {TARGET}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
