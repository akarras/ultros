//! Parser for the game's `.lgb` layer-group files, which place objects into a
//! territory: `bg/<territory bg dir>/level/{planevent,planlive,planmap,bg}.lgb`.
//!
//! Only the parts needed to answer "where does NPC N stand?" are decoded: the
//! layer list, each layer's festival gate, and each instance object's type,
//! transform and, for an EventNPC, its `ENpcBase` id. The `Level` sheet only
//! places NPCs that quests reference; ordinary vendors are placed here.
//!
//! Layout (all little-endian, all offsets relative to the struct that holds
//! them, as in Lumina's `LgbFile`):
//!
//! ```text
//! file:   "LGB1" u32 file_size u32 chunk_count
//! chunk:  "LGP1" u32 chunk_size | u32 layer_group_id i32 name i32 layers i32 layer_count
//!         (`chunk_size` is this header's length, 24 or 32 with two extra words
//!         before `layer_group_id`; `name`/`layers` are relative to the `|`,
//!         i.e. chunk start + 8; the `layers` table holds i32 offsets relative
//!         to the table itself)
//! layer:  u32 id i32 name i32 objects i32 object_count u8[4] flags i32 layer_set_ref
//!         u16 festival u16 festival_phase u8 temporary u8 housing u16 version_mask
//!         u32 reserved i32 ob_set_ref i32 ob_set_ref_count i32 ob_set_enable i32 ob_set_enable_count
//!         (`objects` points at an i32 offset table; entries are relative to that table)
//! object: i32 asset_type u32 instance_id i32 name f32[3] translation f32[3] rotation f32[3] scale
//!         then per-type data; EventNPC (type 8) starts with u32 enpc_base_id
//! ```

use anyhow::{Context, bail};

/// `InstanceObject::asset_type` of an event NPC placement.
pub const ASSET_EVENT_NPC: u32 = 8;

#[derive(Debug, Clone, PartialEq)]
pub struct LgbFile {
    pub layers: Vec<Layer>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    pub id: u32,
    pub name: String,
    /// Seasonal event that enables this layer, or 0 for a permanent layer.
    pub festival_id: u16,
    pub festival_phase_id: u16,
    pub objects: Vec<InstanceObject>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InstanceObject {
    pub asset_type: u32,
    pub instance_id: u32,
    pub name: String,
    /// World position: X, Y (elevation), Z. Map coordinates derive from X and Z.
    pub translation: [f32; 3],
    /// `ENpcBase` id when `asset_type == ASSET_EVENT_NPC`.
    pub enpc_base_id: Option<u32>,
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn bytes(&self, at: usize, len: usize) -> anyhow::Result<&[u8]> {
        self.0
            .get(at..at.checked_add(len).context("offset overflow")?)
            .with_context(|| format!("read of {len} bytes at {at} past end ({})", self.0.len()))
    }
    fn u32(&self, at: usize) -> anyhow::Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(at, 4)?.try_into().unwrap()))
    }
    fn i32(&self, at: usize) -> anyhow::Result<i32> {
        Ok(self.u32(at)? as i32)
    }
    fn u16(&self, at: usize) -> anyhow::Result<u16> {
        Ok(u16::from_le_bytes(self.bytes(at, 2)?.try_into().unwrap()))
    }
    fn f32(&self, at: usize) -> anyhow::Result<f32> {
        Ok(f32::from_bits(self.u32(at)?))
    }
    /// A signed offset stored at `at`, relative to `base`. Layer-table entries
    /// in the 32-byte-header variant point backwards, so negatives are valid.
    fn offset(&self, base: usize, at: usize) -> anyhow::Result<usize> {
        let rel = self.i32(at)?;
        base.checked_add_signed(rel as isize)
            .with_context(|| format!("offset {rel} at {at} points before the file"))
    }
    /// Check that a table of `count` i32 entries starting at `at` lies inside
    /// the file, so a garbage count cannot drive a huge allocation.
    fn table(&self, at: usize, count: u32) -> anyhow::Result<usize> {
        let count = count as usize;
        self.bytes(at, count * 4)
            .with_context(|| format!("table of {count} entries at {at}"))?;
        Ok(count)
    }
    fn cstr(&self, at: usize) -> anyhow::Result<String> {
        let tail = self.0.get(at..).context("string offset past end")?;
        let end = tail
            .iter()
            .position(|&b| b == 0)
            .context("unterminated string")?;
        Ok(String::from_utf8_lossy(&tail[..end]).into_owned())
    }
}

pub fn parse(bytes: &[u8]) -> anyhow::Result<LgbFile> {
    let r = Reader(bytes);
    if r.bytes(0, 4)? != b"LGB1" {
        bail!("not an LGB1 file");
    }
    let chunk_count = r.u32(8)?;
    let mut layers = Vec::new();
    // Most files put the first chunk right after the 12-byte header; a few
    // (housing/dungeon `planner.lgb`) carry 20 more header bytes before it.
    let mut chunk_start = [12usize, 32]
        .into_iter()
        .find(|&at| r.bytes(at, 4).is_ok_and(|m| m == b"LGP1"))
        .context("no LGP1 chunk after the file header")?;
    for _ in 0..chunk_count {
        if r.bytes(chunk_start, 4)? != b"LGP1" {
            bail!("expected LGP1 chunk at {chunk_start}");
        }
        // `chunk_size` is the header's own length: 24 in the common shape,
        // 32 in the variant with two extra words. The name / layer-table /
        // layer-count trio is always the header's last 12 bytes.
        let chunk_size = r.u32(chunk_start + 4)? as usize;
        if chunk_size < 24 {
            bail!("LGP1 header of {chunk_size} bytes at {chunk_start}");
        }
        let base = chunk_start + 8;
        let tail = chunk_start + chunk_size - 12;
        let layer_table = r.offset(base, tail + 4)?;
        let layer_count = r.table(layer_table, r.u32(tail + 8)?)?;
        for i in 0..layer_count {
            let layer_start = r.offset(layer_table, layer_table + i * 4)?;
            layers.push(parse_layer(&r, layer_start)?);
        }
        chunk_start = base + chunk_size;
    }
    Ok(LgbFile { layers })
}

fn parse_layer(r: &Reader, start: usize) -> anyhow::Result<Layer> {
    let id = r.u32(start)?;
    let name = r.cstr(r.offset(start, start + 4)?)?;
    let table = r.offset(start, start + 8)?;
    let count = r.table(table, r.u32(start + 12)?)?;
    let festival_id = r.u16(start + 24)?;
    let festival_phase_id = r.u16(start + 26)?;
    let mut objects = Vec::with_capacity(count);
    for i in 0..count {
        let obj = r.offset(table, table + i * 4)?;
        objects.push(parse_object(r, obj)?);
    }
    Ok(Layer {
        id,
        name,
        festival_id,
        festival_phase_id,
        objects,
    })
}

fn parse_object(r: &Reader, start: usize) -> anyhow::Result<InstanceObject> {
    let asset_type = r.u32(start)?;
    let instance_id = r.u32(start + 4)?;
    let name = r.cstr(r.offset(start, start + 8)?)?;
    let translation = [r.f32(start + 12)?, r.f32(start + 16)?, r.f32(start + 20)?];
    let enpc_base_id = (asset_type == ASSET_EVENT_NPC)
        .then(|| r.u32(start + 48))
        .transpose()?;
    Ok(InstanceObject {
        asset_type,
        instance_id,
        name,
        translation,
        enpc_base_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hand-assemble a one-chunk, one-layer file holding an EventNPC and a
    /// BG object, with every offset relative to its own struct as the game does.
    fn fixture() -> Vec<u8> {
        let mut f = Vec::new();
        let put = |f: &mut Vec<u8>, v: u32| f.extend_from_slice(&v.to_le_bytes());
        f.extend_from_slice(b"LGB1");
        put(&mut f, 0); // file size, patched below
        put(&mut f, 1);
        // chunk header at 12; offsets relative to 20
        f.extend_from_slice(b"LGP1");
        put(&mut f, 0); // chunk size, patched below
        put(&mut f, 7); // layer group id
        put(&mut f, 0); // name (unused)
        put(&mut f, 16); // layer offset table at base+16 == file offset 36
        put(&mut f, 1);
        // layer table
        put(&mut f, 4); // layer at table(36) + 4 == file offset 40
        // layer at 40 (52 bytes); name at +52, objects table at +56
        let layer = f.len();
        assert_eq!(layer, 40);
        put(&mut f, 99); // id
        put(&mut f, 52); // name
        put(&mut f, 56); // object table
        put(&mut f, 2); // object count
        put(&mut f, 0); // flags
        put(&mut f, 0); // layer set ref
        f.extend_from_slice(&5u16.to_le_bytes()); // festival
        f.extend_from_slice(&2u16.to_le_bytes()); // phase
        put(&mut f, 0); // temporary/housing/version mask
        put(&mut f, 0); // reserved
        put(&mut f, 0);
        put(&mut f, 0);
        put(&mut f, 0);
        put(&mut f, 0);
        assert_eq!(f.len() - layer, 52);
        f.extend_from_slice(b"npc\0");
        // object table at layer+56: two entries, relative to the table
        let table = f.len();
        put(&mut f, 8); // obj0 at table + 8
        put(&mut f, 8 + 61); // obj1 after obj0 (52 bytes + "Gontrantput(&mut f, 8 + 60); // obj1 after obj0 (52 bytes + name)")
        let obj0 = f.len();
        assert_eq!(obj0, table + 8);
        put(&mut f, ASSET_EVENT_NPC);
        put(&mut f, 4242);
        put(&mut f, 52); // name right after base id
        for v in [1.5f32, -2.0, 3.25, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0] {
            f.extend_from_slice(&v.to_le_bytes());
        }
        put(&mut f, 1_000_101);
        f.extend_from_slice(b"Gontrant\0");
        assert_eq!(f.len() - obj0, 61);
        let obj1 = f.len();
        put(&mut f, 1); // BG
        put(&mut f, 1);
        put(&mut f, 48);
        for v in [0f32; 9] {
            f.extend_from_slice(&v.to_le_bytes());
        }
        f.extend_from_slice(b"wall\0");
        let _ = obj1;
        let size = f.len() as u32;
        f[4..8].copy_from_slice(&size.to_le_bytes());
        f[16..20].copy_from_slice(&24u32.to_le_bytes()); // header length
        f
    }

    #[test]
    fn parses_layers_objects_and_event_npc_base_ids() {
        let file = parse(&fixture()).unwrap();
        assert_eq!(file.layers.len(), 1);
        let layer = &file.layers[0];
        assert_eq!((layer.id, layer.name.as_str()), (99, "npc"));
        assert_eq!((layer.festival_id, layer.festival_phase_id), (5, 2));
        assert_eq!(layer.objects.len(), 2);
        let npc = &layer.objects[0];
        assert_eq!(npc.asset_type, ASSET_EVENT_NPC);
        assert_eq!(npc.instance_id, 4242);
        assert_eq!(npc.name, "Gontrant");
        assert_eq!(npc.translation, [1.5, -2.0, 3.25]);
        assert_eq!(npc.enpc_base_id, Some(1_000_101));
        let bg = &layer.objects[1];
        assert_eq!(
            (bg.asset_type, bg.name.as_str(), bg.enpc_base_id),
            (1, "wall", None)
        );
    }

    #[test]
    fn rejects_wrong_magic_and_truncation() {
        assert!(parse(b"LGB2....").is_err());
        let mut f = fixture();
        f.truncate(70);
        assert!(parse(&f).is_err());
    }
}
