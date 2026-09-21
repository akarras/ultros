use anyhow::{Context, Result};
use std::{io::Write, path::Path};

pub fn write_startup(data: &xiv_gen::Data, path: &Path) -> Result<usize> {
    let projected = xiv_gen::browser::startup_data(data);
    let raw = rkyv::to_bytes::<_, 1_048_576>(&projected)
        .map_err(|e| anyhow::anyhow!("serializing browser data: {e:?}"))?;
    let mut packed = Vec::new();
    {
        let mut writer = brotli::CompressorWriter::new(&mut packed, 1 << 20, 11, 24);
        writer.write_all(&raw)?;
        writer.flush()?;
        writer.into_inner();
    }
    std::fs::create_dir_all(path.parent().context("startup directory")?)?;
    std::fs::write(path, &packed)?;
    Ok(packed.len())
}
