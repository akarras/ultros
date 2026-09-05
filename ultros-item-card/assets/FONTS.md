# Social card fonts

The renderer embeds these fonts so previews do not depend on the host machine's
installed fonts or a third-party font service. All files use the SIL Open Font
License 1.1; the complete upstream license texts ship beside the fonts.

| Use | Files | Upstream |
| --- | --- | --- |
| English, German and French headlines | `fonts/Outfit-Black.ttf` | [Outfit](https://github.com/Outfitio/Outfit-Fonts/tree/902773808eb372f70fb34e8946dd1ffe604efc79/fonts/ttf) |
| Latin supporting text | `fonts/Outfit-Regular.ttf` | Same Outfit revision |
| Japanese | `fonts/NotoSansJP-{Bold,Regular}.otf` | [Noto Sans CJK Japanese](https://github.com/notofonts/noto-cjk/tree/f8d157532fbfaeda587e826d4cd5b21a49186f7c/Sans/OTF/Japanese) |
| Korean | `fonts/NotoSansKR-{Bold,Regular}.otf` | [Noto Sans CJK Korean](https://github.com/notofonts/noto-cjk/tree/f8d157532fbfaeda587e826d4cd5b21a49186f7c/Sans/OTF/Korean) |
| Simplified Chinese (`cn`) | `fonts/NotoSansSC-{Bold,Regular}.otf` | [Noto Sans CJK Simplified Chinese](https://github.com/notofonts/noto-cjk/tree/f8d157532fbfaeda587e826d4cd5b21a49186f7c/Sans/OTF/SimplifiedChinese) |
| Traditional Chinese (`tc`) | `fonts/NotoSansTC-{Bold,Regular}.otf` | [Noto Sans CJK Traditional Chinese](https://github.com/notofonts/noto-cjk/tree/f8d157532fbfaeda587e826d4cd5b21a49186f7c/Sans/OTF/TraditionalChinese) |

Outfit is Copyright 2021 The Outfit Project Authors. The Noto Sans CJK font
metadata retains its original Copyright 2014–2021 Adobe attribution. Both the
original copyright and license metadata remain in the font files. See
[`OFL-Outfit.txt`](fonts/OFL-Outfit.txt) and
[`OFL-NotoSansCJK.txt`](fonts/OFL-NotoSansCJK.txt).

Outfit is copied unchanged. The Noto fonts are subsets of static CFF OpenType
fonts, which `fontdue` can rasterize. Each region starts from its own upstream
font, preserving locale-specific ideograph forms without depending on `locl`
OpenType shaping. Use JP for `ja`, KR for `ko`, SC for `cn`, and TC for `tc`.
These are real Bold/Regular weights; no synthetic emboldening or variable-font
axis selection is required. Common Latin branding can always use Outfit.

## Coverage and updates

The subset corpus first decompresses each locale's zlib-compressed
`data/xiv-db/<locale>.rkyv` pack, matching `xiv-gen-db::decompress_data`, then
conservatively scans all UTF-8 character data in the complete decoded archive,
all seven translation catalogs, the card renderer's Rust source, and the shared
frontend `social_card.rs` copy.
The streaming zlib reader matches Rust's handling of the existing packs, which
omit a terminal zlib stream marker; the actual decoded item/job names are
validated by the renderer's Rust coverage tests.
The Japanese face includes all seven archives to support cross-locale glyph
fallback, such as a Korean or Chinese market name on an English page, and
item-name symbols that Outfit lacks.
The archive scan includes inlined short strings, item names and job names;
binary data may retain a few extra mapped
characters, which is preferable to losing a valid game glyph. The script also
retains full common Latin, Greek, Roman numeral, combining-mark, punctuation,
currency, arrow, kana and fullwidth blocks where supported upstream. The
Japanese and Korean faces retain all modern Hangul syllables and Hangul jamo.

The subsets intentionally do not contain every rare CJK ideograph. Regenerate
them when updating game packs, adding translated card copy, or adding coverage
tests with new characters. Renderer glyph-coverage tests should examine actual
localized names and copy, so a missing new character is caught before shipping.
Text outside the bundled data and supported catalogs is not an unlimited font
coverage guarantee. Fontdue directly uses character mappings and outlines;
unused OpenType layout features and hinting are removed to reduce file size.

From the repository root, with Python and Git LFS data installed:

```sh
python -m pip install fonttools==4.60.1
python ultros-item-card/assets/fonts/regenerate.py --download
```

The first invocation downloads immutable upstream revisions to
`<system temporary directory>/ultros-social-card-font-sources`. Every source is
checked against its pinned SHA-256 before use. Pass `--source-cache <directory>`
to retain a cache elsewhere. Subsequent invocations can omit `--download` and
run entirely offline. The script refuses Git LFS pointers instead of producing
incomplete fonts. Runtime rendering and Cargo builds never download fonts.

[`fonts/manifest.json`](fonts/manifest.json) records source URLs, original and
generated SHA-256 hashes, sizes, and Unicode coverage fingerprints. FontTools
is pinned and font timestamps are preserved so unchanged inputs generate the
same bytes. Commit regenerated fonts and the manifest together.
Files are generated under temporary names and then replaced atomically, so a
concurrent build cannot include an incomplete font.
