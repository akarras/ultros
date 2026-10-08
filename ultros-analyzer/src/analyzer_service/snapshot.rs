//! Versioned, gzip-compressed column pages for sequential analyzer restores.
//!
//! A file starts with MAGIC, then listing (1) or sales (2) pages, and ends
//! with a zero tag. Each page holds at most PAGE_KEYS keys. Listings carry a
//! selector and columns of item ids, packed HQ bits, prices and worlds. Sales
//! carry a world and columns of item ids, HQ bits, counts, prices, timestamp
//! seconds and nanoseconds. Variable sale counts avoid storing six unused
//! slots. No map layout, pointers or padding are persisted.
//!
//! Compression and file IO run on a blocking worker. Copy only one page under
//! a read lock, release the lock before IO, and never clone the whole analyzer.
//! Restore stages maps until the end marker AND gzip checksum are validated,
//! so a torn newest file cannot partially overwrite an older good snapshot.

use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufReader, BufWriter, Read, Write},
    ops::Bound::{Excluded, Unbounded},
    path::Path,
};

use anyhow::{Context, Result, bail, ensure};
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use tokio::sync::RwLock;

use super::{
    AnalyzerState, AnySelector, CheapestListingValue, CheapestListings, ItemKey, SALE_HISTORY_SIZE,
    SaleHistory, SaleSummary,
};

const MAGIC: &[u8; 8] = b"ULTCOL01";
const PAGE_KEYS: usize = 1024;
const IO_BUFFER: usize = 64 * 1024;

pub(super) struct Stats {
    pub uncompressed_bytes: u64,
    pub compressed_bytes: u64,
}

struct Counted<W> {
    inner: W,
    bytes: u64,
}

impl<W: Write> Write for Counted<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(bytes)?;
        self.bytes += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

pub(super) fn write(
    path: &Path,
    cheapest: &BTreeMap<AnySelector, RwLock<CheapestListings>>,
    sales: &BTreeMap<i32, RwLock<SaleHistory>>,
) -> Result<Stats> {
    let file = BufWriter::with_capacity(IO_BUFFER, File::create(path)?);
    let gzip = GzEncoder::new(file, Compression::default());
    let mut writer = Counted {
        inner: BufWriter::with_capacity(IO_BUFFER, gzip),
        bytes: 0,
    };
    writer.write_all(MAGIC)?;
    for (selector, lock) in cheapest {
        let mut after = None;
        loop {
            let rows = page(&lock.blocking_read().item_map, after);
            if rows.is_empty() && after.is_some() {
                break;
            }
            write_listings(&mut writer, *selector, &rows)?;
            after = rows.last().map(|(key, _)| *key);
            if after.is_none() {
                break;
            }
        }
    }
    for (world, lock) in sales {
        let mut after = None;
        loop {
            let rows = page(&lock.blocking_read().item_map, after);
            if rows.is_empty() && after.is_some() {
                break;
            }
            write_sales(&mut writer, *world, &rows)?;
            after = rows.last().map(|(key, _)| *key);
            if after.is_none() {
                break;
            }
        }
    }
    writer.write_all(&[0])?;
    let uncompressed_bytes = writer.bytes;
    let gzip = writer.inner.into_inner()?;
    let file = gzip.finish()?.into_inner()?;
    file.sync_all()?;
    Ok(Stats {
        uncompressed_bytes,
        compressed_bytes: file.metadata()?.len(),
    })
}

fn page<T: Clone>(map: &BTreeMap<ItemKey, T>, after: Option<ItemKey>) -> Vec<(ItemKey, T)> {
    map.range((after.map_or(Unbounded, Excluded), Unbounded))
        .take(PAGE_KEYS)
        .map(|(key, value)| (*key, value.clone()))
        .collect()
}

fn write_keys<T>(writer: &mut impl Write, rows: &[(ItemKey, T)]) -> Result<()> {
    writer.write_all(&(rows.len() as u16).to_le_bytes())?;
    for (key, _) in rows {
        writer.write_all(&key.item_id.to_le_bytes())?;
    }
    let mut hq = [0_u8; PAGE_KEYS.div_ceil(8)];
    for (index, (key, _)) in rows.iter().enumerate() {
        hq[index / 8] |= u8::from(key.hq) << (index % 8);
    }
    writer.write_all(&hq[..rows.len().div_ceil(8)])?;
    Ok(())
}

fn write_listings(
    writer: &mut impl Write,
    selector: AnySelector,
    rows: &[(ItemKey, CheapestListingValue)],
) -> Result<()> {
    let (kind, id) = match selector {
        AnySelector::World(id) => (0, id),
        AnySelector::Datacenter(id) => (1, id),
        AnySelector::Region(id) => (2, id),
    };
    writer.write_all(&[1, kind])?;
    writer.write_all(&id.to_le_bytes())?;
    write_keys(writer, rows)?;
    for (_, value) in rows {
        writer.write_all(&value.price.to_le_bytes())?;
    }
    for (_, value) in rows {
        writer.write_all(&value.world_id.to_le_bytes())?;
    }
    Ok(())
}

type Sales = arrayvec::ArrayVec<SaleSummary, SALE_HISTORY_SIZE>;

fn write_sales(writer: &mut impl Write, world: i32, rows: &[(ItemKey, Sales)]) -> Result<()> {
    writer.write_all(&[2])?;
    writer.write_all(&world.to_le_bytes())?;
    write_keys(writer, rows)?;
    for (_, sales) in rows {
        writer.write_all(&[sales.len() as u8])?;
    }
    for sale in rows.iter().flat_map(|(_, sales)| sales) {
        writer.write_all(&sale.price_per_item.to_le_bytes())?;
    }
    for sale in rows.iter().flat_map(|(_, sales)| sales) {
        writer.write_all(&sale.sale_date.and_utc().timestamp().to_le_bytes())?;
    }
    for sale in rows.iter().flat_map(|(_, sales)| sales) {
        writer.write_all(
            &sale
                .sale_date
                .and_utc()
                .timestamp_subsec_nanos()
                .to_le_bytes(),
        )?;
    }
    Ok(())
}

pub(super) fn read(path: &Path) -> Result<AnalyzerState> {
    let file = BufReader::with_capacity(IO_BUFFER, File::open(path)?);
    let reader: Box<dyn Read> = if path.extension().is_some_and(|ext| ext == "gz") {
        Box::new(GzDecoder::new(file))
    } else {
        Box::new(file)
    };
    // Decode gzip in blocks, not once per four-byte field read below.
    let mut reader = BufReader::with_capacity(IO_BUFFER, reader);
    let header = read_array::<8>(&mut reader)?;
    if &header == MAGIC {
        return read_columns(&mut reader);
    }
    ensure!(
        !path.to_string_lossy().ends_with(".columns.gz"),
        "unknown analyzer column snapshot version"
    );
    // One-way compatibility with existing rkyv .bin/.bin.gz snapshots. Stream
    // decompression so even legacy restores do not hold a compressed-file copy.
    let mut bytes = rkyv::AlignedVec::new();
    bytes.extend_from_slice(&header);
    let mut buffer = [0; IO_BUFFER];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    rkyv::from_bytes(&bytes).map_err(|error| anyhow::anyhow!("invalid legacy snapshot: {error}"))
}

fn read_array<const N: usize>(reader: &mut impl Read) -> Result<[u8; N]> {
    let mut bytes = [0; N];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn read_i32(reader: &mut impl Read) -> Result<i32> {
    Ok(i32::from_le_bytes(read_array(reader)?))
}

fn read_keys(reader: &mut impl Read) -> Result<Vec<ItemKey>> {
    let count = u16::from_le_bytes(read_array(reader)?) as usize;
    ensure!(count <= PAGE_KEYS, "snapshot page exceeds key limit");
    let mut keys = Vec::with_capacity(count);
    for _ in 0..count {
        keys.push(ItemKey {
            item_id: read_i32(reader)?,
            hq: false,
        });
    }
    let mut hq = [0; PAGE_KEYS.div_ceil(8)];
    reader.read_exact(&mut hq[..count.div_ceil(8)])?;
    for (index, key) in keys.iter_mut().enumerate() {
        key.hq = hq[index / 8] & (1 << (index % 8)) != 0;
    }
    Ok(keys)
}

fn read_columns(reader: &mut impl Read) -> Result<AnalyzerState> {
    let mut state = AnalyzerState {
        cheapest_items: BTreeMap::new(),
        recent_sale_history: BTreeMap::new(),
    };
    loop {
        match read_array::<1>(reader)?[0] {
            0 => {
                // Reading through EOF validates gzip's CRC/trailer. Merely
                // reaching the end marker would accept a truncated trailer.
                ensure!(reader.read(&mut [0])? == 0, "trailing snapshot data");
                return Ok(state);
            }
            1 => {
                let kind = read_array::<1>(reader)?[0];
                let id = read_i32(reader)?;
                let selector = match kind {
                    0 => AnySelector::World(id),
                    1 => AnySelector::Datacenter(id),
                    2 => AnySelector::Region(id),
                    _ => bail!("invalid snapshot selector"),
                };
                let mut rows: Vec<_> = read_keys(reader)?
                    .into_iter()
                    .map(|key| {
                        (
                            key,
                            CheapestListingValue {
                                price: 0,
                                world_id: 0,
                            },
                        )
                    })
                    .collect();
                for (_, value) in &mut rows {
                    value.price = read_i32(reader)?;
                }
                for (_, value) in &mut rows {
                    value.world_id = read_i32(reader)?;
                }
                let map = &mut state.cheapest_items.entry(selector).or_default().item_map;
                for (key, value) in rows {
                    ensure!(
                        map.insert(key, value).is_none(),
                        "duplicate snapshot listing key"
                    );
                }
            }
            2 => {
                let world = read_i32(reader)?;
                let mut rows: Vec<_> = read_keys(reader)?
                    .into_iter()
                    .map(|key| (key, Sales::new()))
                    .collect();
                let mut counts = Vec::with_capacity(rows.len());
                for _ in &rows {
                    let count = read_array::<1>(reader)?[0] as usize;
                    ensure!(
                        count <= SALE_HISTORY_SIZE,
                        "snapshot sale count exceeds capacity"
                    );
                    counts.push(count);
                }
                for ((_, sales), count) in rows.iter_mut().zip(counts) {
                    for _ in 0..count {
                        sales.push(SaleSummary {
                            price_per_item: read_i32(reader)?,
                            sale_date: chrono::NaiveDateTime::MIN,
                        });
                    }
                }
                for sale in rows.iter_mut().flat_map(|(_, sales)| sales) {
                    let seconds = i64::from_le_bytes(read_array(reader)?);
                    sale.sale_date = chrono::DateTime::from_timestamp(seconds, 0)
                        .context("invalid snapshot timestamp")?
                        .naive_utc();
                }
                for sale in rows.iter_mut().flat_map(|(_, sales)| sales) {
                    let nanos = u32::from_le_bytes(read_array(reader)?);
                    sale.sale_date = chrono::DateTime::from_timestamp(
                        sale.sale_date.and_utc().timestamp(),
                        nanos,
                    )
                    .context("invalid snapshot nanoseconds")?
                    .naive_utc();
                }
                let map = &mut state.recent_sale_history.entry(world).or_default().item_map;
                for (key, value) in rows {
                    ensure!(
                        map.insert(key, value).is_none(),
                        "duplicate snapshot sale key"
                    );
                }
            }
            _ => bail!("invalid snapshot page tag"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(keys: usize) -> AnalyzerState {
        let mut state = AnalyzerState {
            cheapest_items: BTreeMap::new(),
            recent_sale_history: BTreeMap::new(),
        };
        for selector in [
            AnySelector::World(1),
            AnySelector::Datacenter(2),
            AnySelector::Region(3),
        ] {
            let map = &mut state.cheapest_items.entry(selector).or_default().item_map;
            for item in 0..keys {
                map.insert(
                    ItemKey {
                        item_id: item as i32,
                        hq: item % 2 == 0,
                    },
                    CheapestListingValue {
                        price: (item as i32 + 1) * 37,
                        world_id: (item % 3) as i32 + 1,
                    },
                );
            }
        }
        state
            .cheapest_items
            .insert(AnySelector::World(99), CheapestListings::default());
        for world in [1, 2] {
            let map = &mut state.recent_sale_history.entry(world).or_default().item_map;
            for item in 0..keys {
                let mut sales = Sales::new();
                for sample in 0..(item % (SALE_HISTORY_SIZE + 1)) {
                    sales.push(SaleSummary {
                        price_per_item: item as i32 * 37 + sample as i32,
                        sale_date: chrono::DateTime::from_timestamp(
                            1_700_000_000 + item as i64 * 60 - sample as i64,
                            123_456_789 + sample as u32,
                        )
                        .unwrap()
                        .naive_utc(),
                    });
                }
                map.insert(
                    ItemKey {
                        item_id: item as i32,
                        hq: item % 2 == 1,
                    },
                    sales,
                );
            }
        }
        state.recent_sale_history.insert(99, SaleHistory::default());
        state
    }

    fn write_fixture(path: &Path, state: &AnalyzerState) -> Stats {
        let cheapest = state
            .cheapest_items
            .iter()
            .map(|(key, value)| (*key, RwLock::new(value.clone())))
            .collect();
        let sales = state
            .recent_sale_history
            .iter()
            .map(|(key, value)| (*key, RwLock::new(value.clone())))
            .collect();
        write(path, &cheapest, &sales).unwrap()
    }

    fn assert_state(expected: &AnalyzerState, actual: &AnalyzerState) {
        assert_eq!(expected.cheapest_items.len(), actual.cheapest_items.len());
        assert_eq!(
            expected.recent_sale_history.len(),
            actual.recent_sale_history.len()
        );
        for (selector, listings) in &expected.cheapest_items {
            let restored = &actual.cheapest_items[selector].item_map;
            assert_eq!(listings.item_map.len(), restored.len());
            for (key, value) in &listings.item_map {
                // CheapestListingValue's PartialEq deliberately ignores world.
                assert_eq!(
                    (value.price, value.world_id),
                    (restored[key].price, restored[key].world_id)
                );
            }
        }
        for (world, history) in &expected.recent_sale_history {
            assert_eq!(history.item_map, actual.recent_sale_history[world].item_map);
        }
    }

    #[test]
    fn column_pages_round_trip_every_selector_partial_sale_buffers_and_subseconds() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("snapshot-1.columns.gz");
        let state = fixture(PAGE_KEYS * 2 + 17);
        write_fixture(&path, &state);
        assert_state(&state, &read(&path).unwrap());
    }

    #[test]
    fn legacy_compressed_and_uncompressed_rkyv_snapshots_still_restore() {
        let dir = tempfile::tempdir().unwrap();
        let state = fixture(17);
        let bytes = rkyv::to_bytes::<_, 256>(&state).unwrap();
        let plain = dir.path().join("snapshot-1.bin");
        std::fs::write(&plain, &bytes).unwrap();
        assert_state(&state, &read(&plain).unwrap());
        let compressed = dir.path().join("snapshot-2.bin.gz");
        let mut encoder =
            GzEncoder::new(File::create(&compressed).unwrap(), Compression::default());
        encoder.write_all(&bytes).unwrap();
        encoder.finish().unwrap();
        assert_state(&state, &read(&compressed).unwrap());
    }

    #[test]
    fn column_snapshot_size_compared_with_legacy_on_a_multiworld_fixture() {
        let dir = tempfile::tempdir().unwrap();
        let state = fixture(PAGE_KEYS * 8 + 1);
        let bytes = rkyv::to_bytes::<_, 256>(&state).unwrap();
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&bytes).unwrap();
        let legacy_gzip = encoder.finish().unwrap();
        let path = dir.path().join("snapshot-1.columns.gz");
        let stats = write_fixture(&path, &state);
        eprintln!(
            "snapshot fixture: legacy raw={} gzip={}; columns raw={} gzip={}",
            bytes.len(),
            legacy_gzip.len(),
            stats.uncompressed_bytes,
            stats.compressed_bytes
        );
        assert!(stats.uncompressed_bytes < bytes.len() as u64);
        assert!(stats.compressed_bytes < legacy_gzip.len() as u64);
        assert_state(&state, &read(&path).unwrap());
    }

    #[test]
    fn gzip_trailer_must_validate_even_after_all_pages_and_the_end_marker() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("snapshot-1.columns.gz");
        write_fixture(&path, &fixture(17));
        let bytes = std::fs::read(&path).unwrap();
        // The decompressed pages are intact; only the checksum/length trailer
        // is torn. Publishing maps before this check would accept bad state.
        std::fs::write(&path, &bytes[..bytes.len() - 4]).unwrap();
        assert!(read(&path).is_err());
        let mut corrupt = bytes;
        let crc = corrupt.len() - 8;
        corrupt[crc] ^= 0xff;
        std::fs::write(&path, corrupt).unwrap();
        assert!(read(&path).is_err());
    }

    #[test]
    fn malformed_pages_cannot_request_unbounded_allocations_or_exceed_sale_capacity() {
        let mut oversized = vec![1, 0];
        oversized.extend_from_slice(&1_i32.to_le_bytes());
        oversized.extend_from_slice(&((PAGE_KEYS + 1) as u16).to_le_bytes());
        assert!(
            read_columns(&mut &oversized[..])
                .unwrap_err()
                .to_string()
                .contains("key limit")
        );

        let mut too_many_sales = vec![2];
        too_many_sales.extend_from_slice(&1_i32.to_le_bytes());
        too_many_sales.extend_from_slice(&1_u16.to_le_bytes());
        too_many_sales.extend_from_slice(&1_i32.to_le_bytes());
        too_many_sales.extend_from_slice(&[0, (SALE_HISTORY_SIZE + 1) as u8]);
        assert!(
            read_columns(&mut &too_many_sales[..])
                .unwrap_err()
                .to_string()
                .contains("sale count")
        );
    }

    #[test]
    fn duplicate_keys_unknown_versions_and_trailing_data_are_rejected() {
        let mut duplicate = Vec::new();
        let rows = [(
            ItemKey {
                item_id: 1,
                hq: true,
            },
            CheapestListingValue {
                price: 100,
                world_id: 1,
            },
        )];
        write_listings(&mut duplicate, AnySelector::World(1), &rows).unwrap();
        write_listings(&mut duplicate, AnySelector::World(1), &rows).unwrap();
        duplicate.push(0);
        assert!(read_columns(&mut &duplicate[..]).is_err());
        assert!(read_columns(&mut &[0, 1][..]).is_err());
        assert!(read_columns(&mut &[99][..]).is_err());

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("snapshot-1.columns.gz");
        let mut encoder = GzEncoder::new(File::create(&path).unwrap(), Compression::default());
        encoder.write_all(b"ULTCOL02\0").unwrap();
        encoder.finish().unwrap();
        assert!(read(&path).unwrap_err().to_string().contains("version"));
    }
}
