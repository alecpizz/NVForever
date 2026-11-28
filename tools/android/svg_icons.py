#!/usr/bin/env python3
"""Compile bounded, original nvf-touch-svg/2 families into typed vector geometry."""
from __future__ import annotations
import argparse
import hashlib
import json
import math
from pathlib import Path
import re
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_MANIFEST = ROOT / "engine/nvforever/assets/touch/manifest.json"
DEFAULT_OUT = ROOT / "engine/nvforever/src/touch/icons_generated.rs"
NS = "{http://www.w3.org/2000/svg}"
MAX_SOURCE_BYTES = 16 * 1024
MAX_ICONS = 64
MAX_FAMILIES = 8
MAX_PRIMITIVES = 64
MAX_COMMANDS = 256
VIEW = 32.0
PAINT_MARGIN = 1.5
NUMBER = re.compile(r"[+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?\Z")
ARITY = {"M": 2, "L": 2, "Q": 4, "C": 6, "Z": 0}
STYLE_FIELDS = {"id", "label", "directory", "strokeWidth", "strokeLinecap", "strokeLinejoin", "fill", "stroke"}


def identity(value: object) -> bool:
    return isinstance(value, str) and re.fullmatch(r"[a-z][a-z0-9-]{0,39}", value) is not None


def label_valid(value: object) -> bool:
    return isinstance(value, str) and ".." not in value and re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9 .-]{0,47}", value) is not None


def root_attributes(style: dict) -> dict[str, str]:
    return {"width": "32", "height": "32", "viewBox": "0 0 32 32", "fill": style["fill"],
            "stroke": style["stroke"], "color": "#ffffff", "fill-rule": "evenodd",
            "stroke-width": f'{style["strokeWidth"]:g}', "stroke-linecap": style["strokeLinecap"],
            "stroke-linejoin": style["strokeLinejoin"]}


def validate_style(style: object) -> None:
    if not isinstance(style, dict) or set(style) != STYLE_FIELDS:
        raise ValueError("Family must declare exactly its identity and bounded paint style")
    if not identity(style["id"]) or style["directory"] != style["id"] or not label_valid(style["label"]):
        raise ValueError("Unsafe family identity, directory or label")
    width = style["strokeWidth"]
    if type(width) not in (float, int) or not math.isfinite(width) or not 0 <= width <= 3:
        raise ValueError("Stroke width must be finite and at most 3 units")
    if style["strokeLinecap"] not in ("round", "square") or style["strokeLinejoin"] not in ("round", "bevel"):
        raise ValueError("Unsupported cap/join vocabulary")
    if style["fill"] not in ("none", "currentColor") or style["stroke"] not in ("none", "currentColor"):
        raise ValueError("Only white currentColor masks or no paint are supported")
    if (style["fill"] == "none" and style["stroke"] == "none") or (style["stroke"] != "none" and width < 1):
        raise ValueError("Family needs visible fill or a non-hairline stroke")


def number(text: str) -> float:
    if not NUMBER.fullmatch(text):
        raise ValueError(f"Invalid numeric token: {text!r}")
    value = float(text)
    if not math.isfinite(value) or not 0.0 <= value <= VIEW:
        raise ValueError("Coordinates must be finite and inside the 32-unit viewBox")
    return value


def numbers(text: str) -> list[float]:
    return [number(part) for part in text.replace(",", " ").split()]


def path_commands(text: str) -> list[tuple[str, list[float]]]:
    tokens, result, pos, drawing, contour = text.replace(",", " ").split(), [], 0, 0, False
    while pos < len(tokens):
        command = tokens[pos]
        if command not in ARITY:
            raise ValueError("Only explicit space-separated absolute M/L/Q/C/Z commands are supported")
        pos += 1
        count = ARITY[command]
        if len(tokens) - pos < count:
            raise ValueError("Truncated SVG path command")
        args = [number(token) for token in tokens[pos:pos + count]]
        pos += count
        if command == "M":
            contour = True
        elif not contour:
            raise ValueError("A path contour must begin with M")
        elif command == "Z":
            contour = False
        else:
            drawing += 1
        result.append((command, args))
        if len(result) > MAX_COMMANDS:
            raise ValueError("Too many SVG path commands")
    if not result or not drawing:
        raise ValueError("An icon path needs a drawn segment")
    return result


def painted_bounds(points: list[float], style: dict, stroke: bool, radius: float = 0.0) -> None:
    extension = style["strokeWidth"] / 2 if stroke else 0
    if stroke and (style["strokeLinecap"] == "square" or style["strokeLinejoin"] == "bevel"):
        extension *= math.sqrt(2)
    if not points or any(v - radius - extension < PAINT_MARGIN or v + radius + extension > VIEW - PAINT_MARGIN for v in points):
        raise ValueError("Painted geometry needs the shared unclipped margin")


def parse_svg(source: bytes, label: str, style: dict) -> list[dict]:
    validate_style(style)
    if not source or len(source) > MAX_SOURCE_BYTES:
        raise ValueError("SVG source exceeds the byte budget")
    if b"<!DOCTYPE" in source.upper() or b"<!ENTITY" in source.upper() or b"&" in source or re.search(rb"<\?(?!xml\s)", source):
        raise ValueError("Entities, declarations and processing instructions are not accepted")
    try:
        root = ET.fromstring(source)
    except ET.ParseError as error:
        raise ValueError(f"Malformed SVG XML: {error}") from error
    if root.tag != NS + "svg" or root.attrib != root_attributes(style):
        raise ValueError("SVG root must use its exact declared family paint profile")
    if root.text and root.text.strip():
        raise ValueError("Unexpected SVG text")
    primitives, title, command_count = [], False, 0
    for child in root:
        if child.tail and child.tail.strip():
            raise ValueError("Unexpected SVG tail text")
        if child.tag == NS + "title":
            if title or primitives or child.attrib or len(child) or child.text != label:
                raise ValueError("Each SVG begins with one matching plain title")
            title = True
            continue
        if not title or len(child) or (child.text and child.text.strip()):
            raise ValueError("Only direct empty geometry follows the title")
        attrs = dict(child.attrib)
        fill, stroke = attrs.pop("fill", style["fill"]), attrs.pop("stroke", style["stroke"])
        if fill not in ("none", "currentColor") or stroke not in ("none", "currentColor"):
            raise ValueError("Primitive paint must be explicit none/currentColor")
        fill, stroke = fill == "currentColor", stroke == "currentColor"
        if not (fill or stroke) or (stroke and style["strokeWidth"] < 1):
            raise ValueError("Primitive has no visible paint or uses an unsupported hairline")
        if child.tag == NS + "path" and set(attrs) == {"d"}:
            commands = path_commands(attrs["d"])
            if fill and (sum(c == "M" for c, _ in commands) != sum(c == "Z" for c, _ in commands) or commands[-1][0] != "Z"):
                raise ValueError("Filled paths must explicitly close each contour")
            painted_bounds([v for _, args in commands for v in args], style, stroke)
            primitive = {"kind": "path", "commands": commands}
            command_count += len(commands)
        elif child.tag == NS + "circle" and set(attrs) == {"cx", "cy", "r"}:
            x, y, radius = (number(attrs[k]) for k in ("cx", "cy", "r"))
            if radius <= 0:
                raise ValueError("Circle radius must be positive")
            painted_bounds([x, y], style, stroke, radius)
            primitive = {"kind": "circle", "values": [x, y, radius]}
            command_count += 1
        elif child.tag == NS + "line" and set(attrs) == {"x1", "y1", "x2", "y2"} and stroke:
            coords = [number(attrs[k]) for k in ("x1", "y1", "x2", "y2")]
            painted_bounds(coords, style, stroke)
            primitive = {"kind": "path", "commands": [("M", coords[:2]), ("L", coords[2:])]}
            command_count += 2
        elif child.tag in {NS + "polyline", NS + "polygon"} and set(attrs) == {"points"}:
            coords = numbers(attrs["points"])
            closed = child.tag == NS + "polygon"
            if len(coords) < (6 if closed else 4) or len(coords) % 2 or (fill and not closed):
                raise ValueError("Polygon requires three pairs; filled polylines are unsupported")
            painted_bounds(coords, style, stroke)
            commands = [("M" if i == 0 else "L", coords[i:i + 2]) for i in range(0, len(coords), 2)]
            if closed:
                commands.append(("Z", []))
            primitive = {"kind": "path", "commands": commands}
            command_count += len(commands)
        else:
            raise ValueError("Unsupported SVG element/attribute; no semantics are silently ignored")
        primitive.update({"fill": fill, "stroke": stroke})
        primitives.append(primitive)
        if len(primitives) > MAX_PRIMITIVES or command_count > MAX_COMMANDS:
            raise ValueError("SVG geometry exceeds its command budget")
    if not title or not primitives:
        raise ValueError("An icon requires its title and visible geometry")
    return primitives


def source_in(directory: Path, name: str) -> bytes:
    path = directory / name
    if path.is_symlink() or path.resolve().parent != directory.resolve() or path.stat().st_size > MAX_SOURCE_BYTES:
        raise ValueError("Owned SVG path/source exceeds its boundary")
    return path.read_bytes()


def compile_manifest(path: Path) -> tuple[list[dict], str]:
    if path.stat().st_size > MAX_SOURCE_BYTES:
        raise ValueError("Manifest exceeds the byte budget")
    manifest = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(manifest, dict) or set(manifest) != {"schema", "profile", "viewBox", "license", "icons", "families"}:
        raise ValueError("Unknown manifest fields")
    if type(manifest["schema"]) is not int or manifest["schema"] != 2 or manifest["profile"] != "nvf-touch-svg/2" or manifest["viewBox"] != [0, 0, 32, 32] or manifest["license"] != "GPL-3.0-or-later":
        raise ValueError("Unknown manifest profile")
    icons, families = manifest["icons"], manifest["families"]
    if not isinstance(icons, list) or not 1 <= len(icons) <= MAX_ICONS or not isinstance(families, list) or not 1 <= len(families) <= MAX_FAMILIES:
        raise ValueError("Icon/family count exceeds the source contract")
    ids = set()
    for index, icon in enumerate(icons):
        if not isinstance(icon, dict) or set(icon) != {"id", "label", "file", "legacyIndex"}:
            raise ValueError("Each semantic icon declares exactly identity, label, file and index")
        if not identity(icon["id"]) or icon["id"] in ids or not label_valid(icon["label"]) or icon["file"] != icon["id"] + ".svg" or type(icon["legacyIndex"]) is not int or icon["legacyIndex"] != index:
            raise ValueError("Unsafe icon identity/path or unstable legacy index")
        ids.add(icon["id"])
    ids, result, digest = set(), [], hashlib.sha256(path.read_bytes().replace(b"\r\n", b"\n"))
    for style in families:
        validate_style(style)
        if style["id"] in ids:
            raise ValueError("Duplicate family identity")
        directory = path.parent / style["directory"]
        if directory.is_symlink() or directory.resolve().parent != path.parent.resolve():
            raise ValueError("Family directory must be directly inside the owned asset directory")
        compiled = []
        for icon in icons:
            source = source_in(directory, icon["file"])
            try:
                primitives = parse_svg(source, icon["label"], style)
            except ValueError as error:
                raise ValueError(f'{style["id"]}/{icon["file"]}: {error}') from error
            compiled.append({"id": icon["id"], "label": icon["label"], "primitives": primitives})
            digest.update(f'{style["id"]}/{icon["file"]}'.encode()); digest.update(source.replace(b"\r\n", b"\n"))
        result.append({**style, "icons": compiled})
        ids.add(style["id"])
    return result, digest.hexdigest()


def rust_number(n: float) -> str:
    value = repr(float(n))
    return value if "." in value or "e" in value else value + ".0"


def rust_geometry(families: list[dict], digest: str) -> str:
    lines = ["// Generated from original owned SVG families. Do not edit.", f'const SVG_SOURCE_SHA256: &str = "{digest}";', "const FAMILIES: &[IconFamily] = &["]
    for family in families:
        cap = "Round" if family["strokeLinecap"] == "round" else "Square"
        join = "Round" if family["strokeLinejoin"] == "round" else "Bevel"
        lines.append(f'    IconFamily {{ id: {json.dumps(family["id"])}, label: {json.dumps(family["label"])}, stroke_width: {rust_number(family["strokeWidth"])}, line_cap: LineCap::{cap}, line_join: LineJoin::{join}, icons: &[')
        for icon in family["icons"]:
            lines.append(f'        IconSource {{ id: {json.dumps(icon["id"])}, label: {json.dumps(icon["label"])}, primitives: &[')
            for primitive in icon["primitives"]:
                if primitive["kind"] == "circle":
                    geometry = "Geometry::Circle(" + ", ".join(map(rust_number, primitive["values"])) + ")"
                else:
                    commands = [f'Verb::{command}' + ("(" + ", ".join(map(rust_number, args)) + ")" if args else "") for command, args in primitive["commands"]]
                    geometry = "Geometry::Path(&[" + ", ".join(commands) + "])"
                lines.append(f'            Primitive {{ geometry: {geometry}, fill: {str(primitive["fill"]).lower()}, stroke: {str(primitive["stroke"]).lower()} }},')
            lines.append("        ] },")
        lines.append("    ] },")
    return "\n".join(lines + ["];", ""])


def preview_svg(manifest_path: Path, family_id: str, backdrop: str) -> str:
    families, digest = compile_manifest(manifest_path)
    family = next((f for f in families if f["id"] == family_id), None)
    if family is None or backdrop not in ("light", "dark", "complex"):
        raise ValueError("Unknown preview family/backdrop")
    ET.register_namespace("", NS[1:-1])
    columns, cell_w, cell_h = 5, 192, 132
    rows = math.ceil(len(family["icons"]) / columns)
    width, height = columns * cell_w + 24, rows * cell_h + 92
    background, ink, muted = ("#eef3f4", "#142c32", "#405b61") if backdrop == "light" else ("#102326", "#f3fbfa", "#a5c8bd")
    parts = [f'<svg xmlns="{NS[1:-1]}" width="{width}" height="{height}" viewBox="0 0 {width} {height}">', f'<rect width="100%" height="100%" fill="{background}"/>']
    if backdrop == "complex":
        for i in range(0, width + height, 56):
            parts.append(f'<path d="M {i} 0 L 0 {i}" stroke="#56716a" stroke-width="18"/>')
        parts.append('<rect width="100%" height="100%" fill="#102326" fill-opacity="0.4"/>')
    parts.append(f'<text x="18" y="28" fill="{ink}" font-family="sans-serif" font-size="18">{family["label"]} / {backdrop} / actual 24, 32, 48 px</text>')
    for i, icon in enumerate(family["icons"]):
        x, y = 12 + (i % columns) * cell_w, 48 + (i // columns) * cell_h
        for size, offset in [(24, 12), (32, 54), (48, 108)]:
            source = manifest_path.parent / family["directory"] / (icon["id"] + ".svg")
            node = ET.fromstring(source.read_bytes())
            # Expand inherited currentColor in the review document. Some standalone
            # SVG previewers ignore nested SVG viewports/currentColor inheritance.
            paint = {key: value for key, value in node.attrib.items() if key not in {"width", "height", "viewBox", "color"}}
            paint.update({"transform": f"translate({x + offset} {y + 48 - size}) scale({size / 32})", "data-icon-size": str(size)})
            group = ET.Element(NS + "g", {key: ink if value == "currentColor" else value for key, value in paint.items()})
            for child in node:
                child.attrib.update({key: ink if value == "currentColor" else value for key, value in child.attrib.items()})
                group.append(child)
            parts.append(ET.tostring(group, encoding="unicode"))
        parts.append(f'<text x="{x + 12}" y="{y + 77}" fill="{ink}" font-family="sans-serif" font-size="13">{icon["label"]}</text>')
        parts.append(f'<text x="{x + 12}" y="{y + 96}" fill="{muted}" font-family="sans-serif" font-size="10">24 / 32 / 48</text>')
    parts.append(f'<text x="18" y="{height - 24}" fill="{ink}" font-family="sans-serif" font-size="11">Original NVForever / GPL-3.0-or-later / synthetic backdrop / not a game capture</text><desc>Source SHA256 {digest}</desc></svg>')
    return "\n".join(parts) + "\n"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--out", type=Path)
    parser.add_argument("--preview", type=Path)
    parser.add_argument("--family", default="precise-line")
    parser.add_argument("--backdrop", choices=["light", "dark", "complex"], default="dark")
    args = parser.parse_args()
    families, digest = compile_manifest(args.manifest)
    output = rust_geometry(families, digest)
    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        if not args.out.is_file() or args.out.read_text(encoding="utf-8") != output:
            args.out.write_text(output, encoding="utf-8", newline="\n")
    if args.preview:
        args.preview.parent.mkdir(parents=True, exist_ok=True)
        args.preview.write_text(preview_svg(args.manifest, args.family, args.backdrop), encoding="utf-8")
    print(f'Owned SVG families: {len(families)}; {sum(len(f["icons"]) for f in families)} icons validated; source {digest}')


if __name__ == "__main__":
    main()
