# SPDX-License-Identifier: MPL-2.0
"""Verify a rendered static site without network access or third-party packages.

The checker deliberately validates the built artifact rather than its source
fragment.  It covers the failure modes that matter for GitHub Pages: missing
entry points, broken repository-base paths, missing local assets and fragments,
generic GitHub 404 output, and a compact set of document/accessibility invariants.
"""

from __future__ import annotations

import argparse
import re
import sys
import tempfile
from dataclasses import dataclass, field
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urlsplit


EXTERNAL_SCHEMES = {
    "blob",
    "data",
    "http",
    "https",
    "javascript",
    "mailto",
    "tel",
}
GITHUB_404_MARKERS = (
    "There isn't a GitHub Pages site here",
    "File not found",
)
CSS_URL = re.compile(r"url\(\s*(['\"]?)(.*?)\1\s*\)", re.IGNORECASE)


@dataclass(frozen=True)
class Reference:
    value: str
    line: int
    attribute: str


@dataclass
class InteractiveElement:
    tag: str
    line: int
    attributes: dict[str, str]
    text: list[str] = field(default_factory=list)


class DocumentParser(HTMLParser):
    """Collect links, identifiers, and basic accessibility metadata."""

    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.references: list[Reference] = []
        self.ids: dict[str, int] = {}
        self.duplicate_ids: list[tuple[str, int]] = []
        self.html_lang = ""
        self.has_charset = False
        self.has_viewport = False
        self.main_count = 0
        self.h1_count = 0
        self.title_parts: list[str] = []
        self.labels_for: set[str] = set()
        self.images: list[tuple[int, dict[str, str]]] = []
        self.canvases: list[tuple[int, dict[str, str]]] = []
        self.form_controls: list[tuple[str, int, dict[str, str]]] = []
        self.interactive: list[InteractiveElement] = []
        self._interactive_stack: list[int] = []
        self._in_title = False
        self._in_style = False

    @staticmethod
    def _attrs(attributes: list[tuple[str, str | None]]) -> dict[str, str]:
        return {name.lower(): value or "" for name, value in attributes}

    def handle_starttag(
        self, tag: str, attributes: list[tuple[str, str | None]]
    ) -> None:
        tag = tag.lower()
        attrs = self._attrs(attributes)
        line = self.getpos()[0]

        element_id = attrs.get("id")
        if element_id:
            if element_id in self.ids:
                self.duplicate_ids.append((element_id, line))
            else:
                self.ids[element_id] = line

        if tag == "html":
            self.html_lang = attrs.get("lang", "").strip()
        elif tag == "meta":
            self.has_charset = self.has_charset or bool(attrs.get("charset", "").strip())
            self.has_viewport = self.has_viewport or (
                attrs.get("name", "").lower() == "viewport"
                and bool(attrs.get("content", "").strip())
            )
        elif tag == "main":
            self.main_count += 1
        elif tag == "h1":
            self.h1_count += 1
        elif tag == "title":
            self._in_title = True
        elif tag == "style":
            self._in_style = True
        elif tag == "label" and attrs.get("for"):
            self.labels_for.add(attrs["for"])
        elif tag == "img":
            self.images.append((line, attrs))
        elif tag == "canvas":
            self.canvases.append((line, attrs))
        elif tag in {"input", "select", "textarea"}:
            self.form_controls.append((tag, line, attrs))

        if tag in {"a", "button"}:
            self.interactive.append(InteractiveElement(tag, line, attrs))
            self._interactive_stack.append(len(self.interactive) - 1)

        for attribute in ("href", "src", "poster", "action"):
            if attribute in attrs and attrs[attribute].strip():
                self.references.append(Reference(attrs[attribute].strip(), line, attribute))

        if attrs.get("srcset"):
            for candidate in attrs["srcset"].split(","):
                value = candidate.strip().split(maxsplit=1)[0]
                if value:
                    self.references.append(Reference(value, line, "srcset"))

        if attrs.get("style"):
            self._collect_css_urls(attrs["style"], line)

    def handle_startendtag(
        self, tag: str, attributes: list[tuple[str, str | None]]
    ) -> None:
        self.handle_starttag(tag, attributes)

    def handle_endtag(self, tag: str) -> None:
        tag = tag.lower()
        if tag == "title":
            self._in_title = False
        elif tag == "style":
            self._in_style = False
        elif tag in {"a", "button"} and self._interactive_stack:
            self._interactive_stack.pop()

    def handle_data(self, data: str) -> None:
        if self._in_title:
            self.title_parts.append(data)
        if self._in_style:
            self._collect_css_urls(data, self.getpos()[0])
        for index in self._interactive_stack:
            self.interactive[index].text.append(data)

    def _collect_css_urls(self, text: str, line: int) -> None:
        for match in CSS_URL.finditer(text):
            value = match.group(2).strip()
            if value:
                self.references.append(Reference(value, line, "css-url"))


def normalize_base_path(value: str) -> str:
    base = "/" + value.strip("/")
    return "/" if base == "/" else base + "/"


def has_accessible_name(attrs: dict[str, str]) -> bool:
    return bool(
        attrs.get("aria-label", "").strip()
        or attrs.get("aria-labelledby", "").strip()
        or attrs.get("title", "").strip()
    )


def parse_document(path: Path, errors: list[str]) -> tuple[DocumentParser, str]:
    try:
        text = path.read_text(encoding="utf-8")
    except (OSError, UnicodeError) as exc:
        errors.append(f"{path}: cannot read as UTF-8 HTML: {exc}")
        return DocumentParser(), ""

    parser = DocumentParser()
    try:
        parser.feed(text)
        parser.close()
    except Exception as exc:  # HTMLParser errors are rare, but must fail closed.
        errors.append(f"{path}: HTML parser failed: {exc}")
    return parser, text


def document_errors(path: Path, parser: DocumentParser) -> list[str]:
    errors: list[str] = []
    prefix = str(path)

    if not parser.html_lang:
        errors.append(f"{prefix}: <html> must declare a non-empty lang attribute")
    if not parser.has_charset:
        errors.append(f"{prefix}: document must declare a character encoding")
    if not parser.has_viewport:
        errors.append(f"{prefix}: document must declare a viewport")
    if not "".join(parser.title_parts).strip():
        errors.append(f"{prefix}: document must have a non-empty <title>")
    if parser.main_count != 1:
        errors.append(f"{prefix}: expected exactly one <main>, found {parser.main_count}")
    if parser.h1_count != 1:
        errors.append(f"{prefix}: expected exactly one <h1>, found {parser.h1_count}")

    for element_id, line in parser.duplicate_ids:
        errors.append(f"{prefix}:{line}: duplicate id #{element_id}")

    for line, attrs in parser.images:
        if "alt" not in attrs:
            errors.append(f"{prefix}:{line}: <img> must provide alt text (empty is valid for decorative images)")

    for line, attrs in parser.canvases:
        if not has_accessible_name(attrs):
            errors.append(f"{prefix}:{line}: <canvas> must have an accessible name")

    for tag, line, attrs in parser.form_controls:
        if tag == "input" and attrs.get("type", "").lower() == "hidden":
            continue
        control_id = attrs.get("id", "")
        if not has_accessible_name(attrs) and control_id not in parser.labels_for:
            errors.append(f"{prefix}:{line}: <{tag}> must have an accessible name or associated label")

    for element in parser.interactive:
        if element.tag == "a" and not element.attributes.get("href", "").strip():
            errors.append(f"{prefix}:{element.line}: <a> must have a non-empty href")
        if not " ".join(element.text).strip() and not has_accessible_name(element.attributes):
            errors.append(f"{prefix}:{element.line}: <{element.tag}> must have an accessible name")

    labelled_ids: list[tuple[str, int]] = []
    for _, line, attrs in parser.form_controls:
        labelled_ids.extend((value, line) for value in attrs.get("aria-labelledby", "").split())
    for line, attrs in parser.canvases:
        labelled_ids.extend((value, line) for value in attrs.get("aria-labelledby", "").split())
    for value, line in labelled_ids:
        if value not in parser.ids:
            errors.append(f"{prefix}:{line}: aria-labelledby references missing id #{value}")

    return errors


def resolve_local_target(
    site_root: Path,
    document: Path,
    reference: Reference,
    base_path: str,
) -> tuple[Path | None, str, str | None]:
    value = reference.value.strip()
    if value.startswith("//"):
        return None, "", None

    parsed = urlsplit(value)
    if parsed.scheme.lower() in EXTERNAL_SCHEMES:
        return None, "", None
    if parsed.scheme:
        return None, "", f"unsupported URL scheme in {value!r}"

    decoded_path = unquote(parsed.path)
    if decoded_path.startswith("/"):
        if base_path != "/" and not decoded_path.startswith(base_path):
            return None, "", (
                f"root-relative path {decoded_path!r} escapes repository base {base_path!r}"
            )
        relative = decoded_path[len(base_path) :] if base_path != "/" else decoded_path[1:]
        target = site_root / relative
    elif decoded_path:
        target = document.parent / decoded_path
    else:
        target = document

    target = target.resolve()
    if not target.is_relative_to(site_root):
        return None, "", f"path {value!r} escapes the rendered site"
    if decoded_path.endswith("/") or target.is_dir():
        target = target / "index.html"
    return target, unquote(parsed.fragment), None


def verify_site(site: Path, base_path: str, expected_markers: list[str]) -> list[str]:
    errors: list[str] = []
    root = site.resolve()
    index = root / "index.html"

    if not root.is_dir():
        return [f"{root}: rendered site directory does not exist"]
    if not index.is_file() or index.stat().st_size == 0:
        return [f"{index}: non-empty Pages entry point is required"]

    html_files = sorted(path for path in root.rglob("*.html") if path.is_file())
    if not html_files:
        return [f"{root}: rendered site contains no HTML documents"]

    documents: dict[Path, tuple[DocumentParser, str]] = {}
    for path in html_files:
        parsed, text = parse_document(path, errors)
        documents[path.resolve()] = (parsed, text)
        errors.extend(document_errors(path.relative_to(root), parsed))

    index_text = documents.get(index.resolve(), (DocumentParser(), ""))[1]
    for marker in GITHUB_404_MARKERS:
        if marker.casefold() in index_text.casefold():
            errors.append(f"index.html: contains generic error marker {marker!r}")
    for marker in expected_markers:
        if marker not in index_text:
            errors.append(f"index.html: expected content marker not found: {marker!r}")

    normalized_base = normalize_base_path(base_path)
    for document, (parsed, _) in documents.items():
        display = document.relative_to(root)
        for reference in parsed.references:
            target, fragment, problem = resolve_local_target(
                root, document, reference, normalized_base
            )
            if problem:
                errors.append(f"{display}:{reference.line}: {problem}")
                continue
            if target is None:
                continue
            if not target.is_file():
                errors.append(
                    f"{display}:{reference.line}: {reference.attribute} target not found: {reference.value!r}"
                )
                continue
            if target.stat().st_size == 0:
                errors.append(
                    f"{display}:{reference.line}: {reference.attribute} target is empty: {reference.value!r}"
                )
                continue
            if fragment and target.suffix.lower() in {".htm", ".html"}:
                target_document = documents.get(target.resolve())
                if target_document is None:
                    target_document = parse_document(target, errors)
                    documents[target.resolve()] = target_document
                if fragment not in target_document[0].ids:
                    errors.append(
                        f"{display}:{reference.line}: fragment target not found: {reference.value!r}"
                    )

    return errors


def self_test() -> None:
    """Exercise both success and deliberately broken-site paths."""

    with tempfile.TemporaryDirectory(prefix="pages-verifier-") as temporary:
        root = Path(temporary)
        (root / "asset.txt").write_text("asset\n", encoding="utf-8")
        (root / "about.html").write_text(
            """<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>About</title></head>
<body><main><h1 id="about">About</h1></main></body></html>
""",
            encoding="utf-8",
        )
        (root / "index.html").write_text(
            """<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>Pilot</title></head>
<body><main><h1>Pilot marker</h1><a href="about.html#about">About</a><a href="asset.txt">Asset</a></main></body></html>
""",
            encoding="utf-8",
        )

        valid_errors = verify_site(root, "/pilot/", ["Pilot marker"])
        if valid_errors:
            raise AssertionError(f"known-good fixture failed: {valid_errors}")

        (root / "index.html").write_text(
            """<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>Broken</title></head>
<body><main><h1>Broken</h1><a href="missing.png">Missing</a></main></body></html>
""",
            encoding="utf-8",
        )
        broken_errors = verify_site(root, "/pilot/", [])
        if not any("target not found" in error for error in broken_errors):
            raise AssertionError("broken-link positive control was not detected")

        (root / "index.html").write_text(
            """<!doctype html><html><head><title>Broken accessibility</title></head>
<body><h1>Broken</h1><img src="asset.txt"></body></html>
""",
            encoding="utf-8",
        )
        accessibility_errors = verify_site(root, "/pilot/", [])
        required = ("lang attribute", "character encoding", "viewport", "exactly one <main>", "provide alt text")
        if not all(any(item in error for error in accessibility_errors) for item in required):
            raise AssertionError("accessibility positive control was not fully detected")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("site", nargs="?", type=Path, help="rendered site directory")
    parser.add_argument("--base-path", default="/", help="Pages repository base path")
    parser.add_argument(
        "--expected-marker",
        action="append",
        default=[],
        help="literal text that must occur in the rendered entry point (repeatable)",
    )
    parser.add_argument("--self-test", action="store_true", help="run planted pass/fail controls")
    args = parser.parse_args()

    if args.self_test:
        self_test()
        print("pages verifier self-test passed (valid fixture + broken-link/accessibility controls)")
        return 0
    if args.site is None:
        parser.error("site is required unless --self-test is used")

    errors = verify_site(args.site, args.base_path, args.expected_marker)
    if errors:
        for error in errors:
            print(f"ERROR: {error}", file=sys.stderr)
        print(f"Pages artifact verification failed with {len(errors)} error(s)", file=sys.stderr)
        return 1

    print(f"Pages artifact verified: {args.site} (base path {normalize_base_path(args.base_path)})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
