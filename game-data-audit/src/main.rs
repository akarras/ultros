//! Read-only pack measurements. Experiments change in-memory clones, never the
//! supplied pack. See README.md for the distinction between size and liveness.
use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::HashSet, io::Read, path::PathBuf};
use xiv_gen::Data;

const USAGE: &str = "game-data-audit --pack PATH --encoding brotli|zlib [--quality 0..11] [--columns] [--table NAME] [--experiment NAME|all] [--output PATH]\n\
Default output compression: Brotli q5, lgwin 24. Use --quality 11 for production comparisons.\n\
Experiments: unused_fc_tables, generation_columns, duplicate_shop_item, duplicate_leve_arrays, item_icon_ids, item_descriptions, referenced_npcs, deferred_details";

#[derive(Debug)]
struct Options {
    pack: PathBuf,
    encoding: String,
    quality: u32,
    columns: bool,
    table: Option<String>,
    experiments: Vec<String>,
    output: Option<PathBuf>,
}

impl Options {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self> {
        let mut args = args.into_iter();
        let mut pack = None;
        let mut encoding = None;
        let mut options = Self {
            pack: PathBuf::new(),
            encoding: String::new(),
            quality: 5,
            columns: false,
            table: None,
            experiments: Vec::new(),
            output: None,
        };
        while let Some(arg) = args.next() {
            if arg == "--columns" {
                options.columns = true;
                continue;
            }
            if arg == "--help" || arg == "-h" {
                bail!(USAGE);
            }
            let value = args
                .next()
                .with_context(|| format!("missing value for {arg}"))?;
            match arg.as_str() {
                "--pack" => pack = Some(PathBuf::from(value)),
                "--encoding" => {
                    ensure!(
                        matches!(value.as_str(), "brotli" | "zlib"),
                        "encoding must be brotli or zlib"
                    );
                    encoding = Some(value);
                }
                "--quality" => {
                    options.quality = value.parse()?;
                    ensure!(options.quality <= 11, "quality must be 0..11");
                }
                "--table" => options.table = Some(value),
                "--experiment" => {
                    if value == "all" {
                        options
                            .experiments
                            .extend(EXPERIMENTS.iter().map(|s| s.to_string()));
                    } else {
                        ensure!(
                            EXPERIMENTS.contains(&value.as_str()),
                            "unknown experiment {value}"
                        );
                        options.experiments.push(value);
                    }
                }
                "--output" => options.output = Some(PathBuf::from(value)),
                _ => bail!("unknown argument {arg}"),
            }
        }
        options.pack = pack.context("--pack is required")?;
        options.encoding =
            encoding.context("--encoding is required; input encoding is never guessed")?;
        options.experiments.sort();
        options.experiments.dedup();
        Ok(options)
    }

    fn includes_table(&self, name: &str) -> bool {
        self.table.as_deref().is_none_or(|filter| filter == name)
    }
}

#[derive(Serialize)]
struct Measurement {
    name: String,
    kind: &'static str,
    records: usize,
    rkyv_bytes: usize,
    brotli_bytes: usize,
}

#[derive(Serialize)]
struct Experiment {
    name: String,
    interpretation: &'static str,
    rkyv_bytes: usize,
    brotli_bytes: usize,
    saved_brotli_bytes: i64,
}

#[derive(Serialize)]
struct Report {
    format_version: u32,
    pack: String,
    pack_sha256: String,
    schema_sha256: String,
    input_encoding: String,
    input_bytes: usize,
    decoded_bytes: usize,
    output_compression: String,
    baseline: Measurement,
    measurements: Vec<Measurement>,
    experiments: Vec<Experiment>,
    notes: Vec<&'static str>,
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn compress(bytes: &[u8], quality: u32) -> Result<Vec<u8>> {
    let params = brotli::enc::BrotliEncoderParams {
        quality: quality as i32,
        lgwin: 24,
        ..Default::default()
    };
    let mut packed = Vec::new();
    brotli::BrotliCompress(&mut std::io::Cursor::new(bytes), &mut packed, &params)?;
    Ok(packed)
}

fn decode(bytes: &[u8], encoding: &str) -> Result<(Data, usize)> {
    ensure!(
        !bytes.starts_with(b"version https://git-lfs.github.com/spec"),
        "input is a Git LFS pointer, not a pack; fetch the LFS content first"
    );
    ensure!(!bytes.is_empty(), "input pack is empty");
    let mut raw = Vec::new();
    match encoding {
        "brotli" => {
            brotli::Decompressor::new(bytes, 4096)
                .read_to_end(&mut raw)
                .context("decoding Brotli pack")?;
        }
        "zlib" => {
            flate2::read::ZlibDecoder::new(bytes)
                .read_to_end(&mut raw)
                .context("decoding legacy zlib pack")?;
        }
        _ => bail!("unsupported encoding {encoding}"),
    }
    let mut aligned = rkyv::AlignedVec::with_capacity(raw.len());
    aligned.extend_from_slice(&raw);
    let data = rkyv::from_bytes::<Data>(&aligned)
        .map_err(|e| anyhow::anyhow!("pack does not match this checkout's Data schema: {e:?}"))?;
    Ok((data, raw.len()))
}

fn archive(data: &Data) -> Result<rkyv::AlignedVec> {
    rkyv::to_bytes::<_, 256>(data).map_err(|e| anyhow::anyhow!("serializing Data: {e:?}"))
}

fn measure(
    name: &str,
    kind: &'static str,
    records: usize,
    bytes: &[u8],
    quality: u32,
) -> Result<Measurement> {
    Ok(Measurement {
        name: name.into(),
        kind,
        records,
        rkyv_bytes: bytes.len(),
        brotli_bytes: compress(bytes, quality)?.len(),
    })
}

fn snapshot<T: Clone>(value: &T) -> T {
    value.clone()
}

include!(concat!(env!("OUT_DIR"), "/inventory.rs"));

const EXPERIMENTS: &[&str] = &[
    "unused_fc_tables",
    "generation_columns",
    "duplicate_shop_item",
    "duplicate_leve_arrays",
    "item_icon_ids",
    "item_descriptions",
    "referenced_npcs",
    "deferred_details",
];

fn retain_referenced_npcs(data: &mut Data) {
    let mut ids: HashSet<_> = data
        .gil_shop_npcs
        .values()
        .chain(data.special_shop_npcs.values())
        .chain(data.collectables_shop_npcs.values())
        .chain(data.leve_issuers.values())
        .flatten()
        .copied()
        .collect();
    ids.extend(data.npc_placements.keys().copied());
    data.e_npc_residents.retain(|id, _| ids.contains(id));
}

fn apply_experiment(data: &mut Data, name: &str) -> Result<&'static str> {
    match name {
        "unused_fc_tables" => {
            data.company_craft_drafts.clear();
            data.company_craft_draft_categorys.clear();
            data.company_craft_types.clear();
            Ok("Empty three candidate-unused tables; keeps wire schema and empty table headers.")
        }
        "generation_columns" => {
            for row in data.e_npc_residents.values_mut() {
                row.map = 0;
            }
            for row in data.territory_types.values_mut() {
                row.bg.clear();
            }
            for row in data.maps.values_mut() {
                row.offset_x = 0;
                row.offset_y = 0;
                row.place_name_region = 0;
            }
            Ok(
                "Neutralize candidate generation-only columns. Fixed-width slots remain; this is NOT physical column removal.",
            )
        }
        "duplicate_shop_item" => {
            ensure!(
                data.special_shops
                    .values()
                    .all(|row| row.item == row.item_receive_0),
                "SpecialShop.item differs from item_receive_0; refusing duplicate-column experiment"
            );
            for row in data.special_shops.values_mut() {
                row.item.clear();
            }
            Ok(
                "Clear the duplicate SpecialShop.item vector after verifying equality in every row. Consumers must migrate before removal.",
            )
        }
        "duplicate_leve_arrays" => {
            for row in data.leve_reward_items.values_mut() {
                ensure!(
                    row.leve_reward_item_group
                        == [
                            row.leve_reward_item_group_0,
                            row.leve_reward_item_group_1,
                            row.leve_reward_item_group_2,
                            row.leve_reward_item_group_3,
                            row.leve_reward_item_group_4,
                            row.leve_reward_item_group_5,
                            row.leve_reward_item_group_6,
                            row.leve_reward_item_group_7
                        ],
                    "LeveRewardItem array disagrees with scalar columns"
                );
                row.leve_reward_item_group = [0; 8];
            }
            for row in data.leve_reward_item_groups.values_mut() {
                ensure!(
                    row.item
                        == [
                            row.item_0, row.item_1, row.item_2, row.item_3, row.item_4, row.item_5,
                            row.item_6, row.item_7, row.item_8
                        ],
                    "LeveRewardItemGroup array disagrees with scalar columns"
                );
                row.item = [0; 9];
            }
            Ok(
                "Verify redundant leve arrays equal the scalar columns, then zero arrays. Fixed-width array slots remain.",
            )
        }
        "item_icon_ids" => {
            for row in data.items.values_mut() {
                row.icon = 0;
            }
            Ok(
                "Zero source icon IDs. Browser icon URLs and server search use item IDs; extraction still needs source icon IDs. Fixed-width slots remain.",
            )
        }
        "item_descriptions" => {
            for row in data.items.values_mut() {
                row.description.clear();
            }
            Ok(
                "Defer descriptions; tooltips and item pages need an alternative data source. String slots remain.",
            )
        }
        "referenced_npcs" => {
            retain_referenced_npcs(data);
            Ok(
                "Keep NPCs referenced by shop indexes, leve issuers, or placements. Arbitrary NPC URLs need a fallback before shipping.",
            )
        }
        "deferred_details" => {
            apply_experiment(data, "item_descriptions")?;
            retain_referenced_npcs(data);
            Ok(
                "Combined descriptions/NPC experiment, measured together rather than summing separate savings. Requires on-demand detail loading.",
            )
        }
        _ => bail!("unknown experiment {name}"),
    }
}

fn run(options: Options) -> Result<()> {
    let bytes = std::fs::read(&options.pack)
        .with_context(|| format!("reading {}", options.pack.display()))?;
    let (data, decoded_bytes) = decode(&bytes, &options.encoding)?;
    eprintln!(
        "Decoded {} bytes; measuring Brotli q{} / lgwin 24",
        decoded_bytes, options.quality
    );
    let baseline = measure(
        "complete_pack",
        "baseline",
        1,
        &archive(&data)?,
        options.quality,
    )?;
    let measurements = inventory(&data, &options)?;
    ensure!(
        !measurements.is_empty(),
        "unknown table {:?}",
        options.table
    );
    let mut experiments = Vec::new();
    for name in &options.experiments {
        eprintln!("Measuring {name}");
        let mut candidate = data.clone();
        let interpretation = apply_experiment(&mut candidate, name)?;
        let raw = archive(&candidate)?;
        let brotli_bytes = compress(&raw, options.quality)?.len();
        experiments.push(Experiment {
            name: name.clone(),
            interpretation,
            rkyv_bytes: raw.len(),
            brotli_bytes,
            saved_brotli_bytes: baseline.brotli_bytes as i64 - brotli_bytes as i64,
        });
    }
    let report = Report {
        format_version: 1,
        pack: options.pack.display().to_string(),
        pack_sha256: digest(&bytes),
        schema_sha256: digest(include_bytes!("../../xiv-gen/src/lib.rs")),
        input_encoding: options.encoding,
        input_bytes: bytes.len(),
        decoded_bytes,
        output_compression: format!("brotli q{} lgwin24", options.quality),
        baseline,
        measurements,
        experiments,
        notes: vec![
            "Table sizes are independently serialized/compressed; they are not additive shares of the original pack.",
            "Column samples are sorted (map key, field value) vectors; their sizes rank payloads, not physical column-removal savings.",
            "Experiment savings compare against a reserialized whole-pack baseline using identical Brotli settings, regardless of input encoding.",
            "Measurements establish size, not liveness. No field or row is safe to remove solely because a search or test did not access it.",
        ],
    };
    let json = serde_json::to_string_pretty(&report)?;
    if let Some(output) = options.output {
        ensure!(
            output != options.pack,
            "output must not overwrite the input pack"
        );
        if output.exists() {
            ensure!(
                std::fs::canonicalize(&output)? != std::fs::canonicalize(&options.pack)?,
                "output must not overwrite the input pack"
            );
        }
        std::fs::write(&output, format!("{json}\n"))?;
        eprintln!("Report: {}", output.display());
    } else {
        println!("{json}");
    }
    Ok(())
}

fn main() -> Result<()> {
    if std::env::args().any(|arg| arg == "--help" || arg == "-h") {
        println!("{USAGE}");
        return Ok(());
    }
    run(Options::parse(std::env::args().skip(1))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use xiv_gen::{ENpcResident, ENpcResidentId, GilShopId};

    #[test]
    fn refuses_pointer_and_wrong_schema() {
        assert!(
            decode(b"version https://git-lfs.github.com/spec/v1\n", "brotli")
                .unwrap_err()
                .to_string()
                .contains("LFS")
        );
        let bytes = compress(b"not a Data archive", 1).unwrap();
        assert!(
            decode(&bytes, "brotli")
                .unwrap_err()
                .to_string()
                .contains("schema")
        );
    }

    #[test]
    fn both_input_encodings_decode_the_same_schema() {
        use std::io::Write;
        let data = Data::default();
        let raw = archive(&data).unwrap();
        let br = compress(&raw, 1).unwrap();
        assert_eq!(decode(&br, "brotli").unwrap().0, data);
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
        z.write_all(&raw).unwrap();
        assert_eq!(decode(&z.finish().unwrap(), "zlib").unwrap().0, data);
    }

    #[test]
    fn npc_pruning_preserves_each_reference_source() {
        let mut data = Data::default();
        for id in 1..=6 {
            data.e_npc_residents.insert(
                ENpcResidentId(id),
                ENpcResident {
                    key_id: ENpcResidentId(id),
                    singular: id.to_string(),
                    map: 0,
                },
            );
        }
        data.gil_shop_npcs
            .insert(GilShopId(1), vec![ENpcResidentId(1)]);
        data.special_shop_npcs
            .insert(xiv_gen::SpecialShopId(1), vec![ENpcResidentId(2)]);
        data.collectables_shop_npcs
            .insert(xiv_gen::CollectablesShopId(1), vec![ENpcResidentId(3)]);
        data.leve_issuers
            .insert(xiv_gen::LeveId(1), vec![ENpcResidentId(4)]);
        data.npc_placements.insert(ENpcResidentId(5), vec![]);
        let original = data.clone();
        retain_referenced_npcs(&mut data);
        assert_eq!(data.e_npc_residents.len(), 5);
        assert!(!data.e_npc_residents.contains_key(&ENpcResidentId(6)));
        assert_eq!(original.e_npc_residents.len(), 6);
    }

    #[test]
    fn cli_rejects_unknown_experiments_and_quality() {
        for extra in [["--experiment", "typo"], ["--quality", "12"]] {
            assert!(
                Options::parse(
                    ["--pack", "x", "--encoding", "brotli", extra[0], extra[1]].map(str::to_string)
                )
                .is_err()
            );
        }
    }

    #[test]
    fn duplicate_experiment_rejects_disagreeing_source_columns() {
        let mut data = Data::default();
        // Use the real wire type, including all fields, so an omitted column
        // in the fixture cannot masquerade as an empty/default source value.
        let shop: xiv_gen::SpecialShop = serde_json::from_value(serde_json::json!({
            "key_id": 1, "name": "example", "item": [123],
            "item_receive_0": [456], "count_receive_0": [1],
            "item_receive_1": [], "count_receive_1": [],
            "item_cost_0": [], "count_cost_0": [],
            "item_cost_1": [], "count_cost_1": [],
            "item_cost_2": [], "count_cost_2": []
        }))
        .unwrap();
        data.special_shops.insert(shop.key_id, shop);
        let before = data.clone();
        assert!(apply_experiment(&mut data, "duplicate_shop_item").is_err());
        assert_eq!(data, before);
        data.special_shops
            .get_mut(&xiv_gen::SpecialShopId(1))
            .unwrap()
            .item = vec![456];
        apply_experiment(&mut data, "duplicate_shop_item").unwrap();
        let shop = &data.special_shops[&xiv_gen::SpecialShopId(1)];
        assert!(shop.item.is_empty());
        assert_eq!(shop.item_receive_0, vec![456]);
    }

    #[test]
    fn generated_inventory_covers_every_serialized_table() {
        let options = Options::parse(
            [
                "--pack",
                "x",
                "--encoding",
                "brotli",
                "--quality",
                "0",
                "--columns",
            ]
            .map(str::to_string),
        )
        .unwrap();
        let data = Data::default();
        let rows = inventory(&data, &options).unwrap();
        let fields = serde_json::to_value(&data).unwrap();
        let tables: HashSet<_> = rows
            .iter()
            .filter(|r| r.kind == "table")
            .map(|r| r.name.as_str())
            .collect();
        assert_eq!(
            tables,
            fields
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<HashSet<_>>()
        );
        assert!(rows.iter().any(|r| r.name == "items.description"));
        assert!(rows.iter().any(|r| r.name == "gil_shop_items.availability"));
    }
}
