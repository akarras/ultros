#!/usr/bin/env python3
"""Rebuild the committed social-card fonts from pinned, OFL-licensed sources.

Requires fonttools==4.60.1. Downloads happen only with --download; normal builds
are offline. The source cache is outside the repository by default.
"""

from __future__ import annotations

import argparse
import gc
import hashlib
import json
from pathlib import Path
import shutil
import tempfile
from urllib.request import urlopen
import zlib

import fontTools
from fontTools import subset
from fontTools.ttLib import TTFont


HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
OUTFIT_REVISION = "902773808eb372f70fb34e8946dd1ffe604efc79"
NOTO_REVISION = "f8d157532fbfaeda587e826d4cd5b21a49186f7c"
REGIONS = {
    "JP": ("ja", "Japanese"),
    "KR": ("ko", "Korean"),
    "SC": ("cn", "SimplifiedChinese"),
    "TC": ("tc", "TraditionalChinese"),
}
SOURCE_SHA256 = {
    "Outfit-Black.ttf": "1c033aa4d2ed288e5a9fb6e379bbe52e2c5b8ce0fd06bcee928c14f9bccbca4c",
    "Outfit-Regular.ttf": "3b64ac4f6ab6a8eebddd4b0bc03c811c43602e11e176382ab0ee6be615ab861b",
    "OFL-Outfit.txt": "c676351bf8576b9aba743cd5eaa8c0e7ee0d51f805d720447b4df4ddb6a2e416",
    "OFL-NotoSansCJK.txt": "6a73f9541c2de74158c0e7cf6b0a58ef774f5a780bf191f2d7ec9cc53efe2bf2",
    "NotoSansJP-Bold.otf": "e53dcb0dcb2922e45d01aae1ebd2f382bb81d4229b18b6b883bd170678af1f76",
    "NotoSansJP-Regular.otf": "68a3fc98800b2a27b371f2fb79991daf3633bd89309d4ffaa6946fd587f375b5",
    "NotoSansKR-Bold.otf": "26d0c6748500a0444844280b308f5b62c7ae92ac6c6ac88148e502dd211eb52a",
    "NotoSansKR-Regular.otf": "6bcb2a0703aa137e874fc2dffa85f6c21ba9a67fa329e81b8c801663af7e992a",
    "NotoSansSC-Bold.otf": "b5f0d1a190a7f9b43c310a8850630af12553df32c4c050543f9059732d9b4c0a",
    "NotoSansSC-Regular.otf": "2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b",
    "NotoSansTC-Bold.otf": "3ee160e5015106e3ec1a394301df54fa9bbbf8a251519984aec5c0abc50840c0",
    "NotoSansTC-Regular.otf": "dce08bd4fd91aa8aa76ed8fea4b694c2dfb8550f67871e326843212ddbeb88b4",
}


def sources() -> dict[str, str]:
    outfit = f"https://raw.githubusercontent.com/Outfitio/Outfit-Fonts/{OUTFIT_REVISION}"
    noto = f"https://raw.githubusercontent.com/notofonts/noto-cjk/{NOTO_REVISION}"
    result = {
        "Outfit-Black.ttf": f"{outfit}/fonts/ttf/Outfit-Black.ttf",
        "Outfit-Regular.ttf": f"{outfit}/fonts/ttf/Outfit-Regular.ttf",
        "OFL-Outfit.txt": f"{outfit}/OFL.txt",
        "OFL-NotoSansCJK.txt": f"{noto}/Sans/LICENSE",
    }
    for region, (_, directory) in REGIONS.items():
        for weight in ("Bold", "Regular"):
            filename = f"NotoSans{region}-{weight}.otf"
            result[filename] = (
                f"{noto}/Sans/OTF/{directory}/NotoSansCJK{region.lower()}-{weight}.otf"
            )
    return result


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def coverage(locale: str) -> set[int]:
    # The .rkyv packs are zlib compressed (xiv-gen-db::decompress_data). Once
    # decompressed, archived strings preserve their UTF-8 bytes. Scanning them
    # retains a conservative superset of the item/job names, including inline
    # short strings, without requiring a second rkyv parser or a Rust build.
    codepoints = set()
    # The Japanese face also supplies cross-locale glyph fallback, including
    # Korean/Chinese market names on an English page, so include every locale.
    languages = ("en", "de", "fr", "ja", "ko", "cn", "tc") if locale == "ja" else (locale,)
    for language in languages:
        archive = ROOT / "data" / "xiv-db" / f"{language}.rkyv"
        archive_bytes = archive.read_bytes()
        if archive_bytes.startswith(b"version https://git-lfs.github.com/spec/"):
            raise RuntimeError(f"{archive}: run git lfs pull before regenerating fonts")
        # Match flate2's streaming reader: the existing packs yield their full
        # archive without a terminal zlib stream marker. The one-shot Python
        # zlib.decompress requires that marker and rejects those valid inputs.
        decoder = zlib.decompressobj()
        decompressed = decoder.decompress(archive_bytes) + decoder.flush()
        if not decompressed or decoder.unconsumed_tail or decoder.unused_data:
            raise RuntimeError(f"{archive}: unexpected compressed game-data format")
        codepoints.update(map(ord, decompressed.decode("utf-8", errors="ignore")))
    for path in sorted((ROOT / "ultros-frontend" / "ultros-app" / "locales").glob("*.json")):
        # Decode JSON so escaped Unicode is covered too.
        catalog = json.loads(path.read_text(encoding="utf-8"))
        codepoints.update(map(ord, json.dumps(catalog, ensure_ascii=False)))
    for path in sorted((ROOT / "ultros-item-card" / "src").glob("*.rs")):
        codepoints.update(map(ord, path.read_text(encoding="utf-8")))
    shared_copy = ROOT / "ultros-frontend" / "ultros-app" / "src" / "social_card.rs"
    if shared_copy.exists():
        codepoints.update(map(ord, shared_copy.read_text(encoding="utf-8")))

    # Keep full common script blocks for future UI copy and tests. Ideographs
    # come from the complete archive/catalog corpus; they are not reduced to
    # the small set of sample names displayed in the design references.
    ranges = [
        (0x0020, 0x024F),  # Basic/extended Latin, including French and German.
        (0x0300, 0x036F),  # Combining diacritics.
        (0x0370, 0x03FF),  # Greek letters used in item names.
        (0x2000, 0x206F),  # General punctuation, including ellipsis.
        (0x20A0, 0x20CF),  # Currency symbols.
        (0x2100, 0x218F),  # Letterlike symbols and Roman numerals.
        (0x2190, 0x21FF),  # Arrows.
        (0x3000, 0x30FF),  # CJK punctuation, hiragana and katakana.
        (0x31F0, 0x31FF),  # Katakana phonetic extensions.
        (0xFF00, 0xFFEF),  # Full/halfwidth forms.
    ]
    if locale in ("ja", "ko"):
        ranges.extend([
            (0x1100, 0x11FF),  # Hangul jamo.
            (0x3130, 0x318F),  # Hangul compatibility jamo.
            (0xA960, 0xA97F),  # Hangul jamo extended A.
            (0xAC00, 0xD7AF),  # Every modern Hangul syllable.
            (0xD7B0, 0xD7FF),  # Hangul jamo extended B.
        ])
    for first, last in ranges:
        codepoints.update(range(first, last + 1))
    return codepoints


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--download", action="store_true", help="download pinned sources")
    parser.add_argument(
        "--source-cache", type=Path,
        default=Path(tempfile.gettempdir()) / "ultros-social-card-font-sources",
    )
    args = parser.parse_args()
    if fontTools.__version__ != "4.60.1":
        raise RuntimeError("Use fonttools==4.60.1 for reproducible font output")
    args.source_cache.mkdir(parents=True, exist_ok=True)
    manifest = {"fonttools": fontTools.__version__, "files": {}}
    corpora = {region: coverage(locale) for region, (locale, _) in REGIONS.items()}

    for filename, url in sources().items():
        source = args.source_cache / filename
        if args.download and not source.exists():
            with urlopen(url) as response:
                source.write_bytes(response.read())
        if not source.exists():
            raise RuntimeError(f"Missing {source}; use --download to retrieve pinned sources")
        original = source.read_bytes()
        if digest(original) != SOURCE_SHA256[filename]:
            raise RuntimeError(f"{source}: checksum does not match the pinned upstream source")
        info = {"source": url, "source_sha256": digest(original)}
        destination = HERE / filename
        if not filename.startswith("NotoSans"):
            temporary = destination.with_suffix(destination.suffix + ".tmp")
            shutil.copyfile(source, temporary)
            temporary.replace(destination)
        else:
            region = filename.removeprefix("NotoSans").split("-", 1)[0]
            font = TTFont(source, recalcTimestamp=False)
            wanted = corpora[region].intersection(font.getBestCmap())
            options = subset.Options()
            options.layout_features = []  # fontdue uses outlines/cmap directly.
            options.name_IDs = ["*"]     # Preserve attribution and OFL metadata.
            options.name_legacy = True
            options.name_languages = ["*"]
            options.recalc_timestamp = False
            options.hinting = False
            options.canonical_order = True
            sub = subset.Subsetter(options=options)
            sub.populate(unicodes=sorted(wanted))
            sub.subset(font)
            temporary = destination.with_suffix(destination.suffix + ".tmp")
            font.save(temporary, reorderTables=True)
            actual = set(font.getBestCmap())
            if not wanted.issubset(actual):
                raise RuntimeError(f"{filename}: subset lost requested glyphs")
            info["unicode_codepoints"] = len(actual)
            info["codepoint_sha256"] = digest(
                "\n".join(f"{cp:06X}" for cp in sorted(actual)).encode("ascii")
            )
            temporary.replace(destination)
            del font, sub
            gc.collect()
        output = destination.read_bytes()
        info.update({"bytes": len(output), "sha256": digest(output)})
        manifest["files"][filename] = info
        print(f"{filename}: {len(output):,} bytes", flush=True)
    temporary_manifest = HERE / "manifest.json.tmp"
    temporary_manifest.write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    temporary_manifest.replace(HERE / "manifest.json")


if __name__ == "__main__":
    main()
