use std::io::{Read, Write};

use bytes::Bytes;
use loro_common::LoroError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionType {
    None,
    LZ4,
}

impl CompressionType {
    pub fn is_none(&self) -> bool {
        matches!(self, CompressionType::None)
    }
}

impl TryFrom<u8> for CompressionType {
    type Error = LoroError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(CompressionType::None),
            1 => Ok(CompressionType::LZ4),
            _ => Err(LoroError::DecodeError(
                format!("Invalid compression type: {value}").into(),
            )),
        }
    }
}

impl From<CompressionType> for u8 {
    fn from(value: CompressionType) -> Self {
        match value {
            CompressionType::None => 0,
            CompressionType::LZ4 => 1,
        }
    }
}

pub fn compress(w: &mut Vec<u8>, data: &[u8], compression_type: CompressionType) {
    match compression_type {
        CompressionType::None => {
            w.write_all(data).unwrap();
        }
        CompressionType::LZ4 => {
            let mut encoder = lz4_flex::frame::FrameEncoder::new(w);
            encoder.write_all(data).unwrap();
            let _w = encoder.finish().unwrap();
        }
    }
}

pub fn decompress(
    out: &mut Vec<u8>,
    data: Bytes,
    compression_type: CompressionType,
) -> Result<(), LoroError> {
    match compression_type {
        CompressionType::None => {
            if data.len() > 16 * 1024 * 1024 {
                return Err(LoroError::DecodeError("list block exceeds 16 MiB".into()));
            }
            crate::decode_budget::consume(data.len())?;
            out.write_all(&data).unwrap();
            Ok(())
        }
        CompressionType::LZ4 => {
            let mut decoder = lz4_flex::frame::FrameDecoder::new(data.as_ref());
            let mut chunk = [0_u8; 8192];
            loop {
                let length = decoder
                    .read(&mut chunk)
                    .map_err(|e| LoroError::DecodeError(e.to_string().into()))?;
                if length == 0 {
                    break;
                }
                if out.len().saturating_add(length) > 16 * 1024 * 1024 {
                    return Err(LoroError::DecodeError("list block exceeds 16 MiB".into()));
                }
                crate::decode_budget::consume(length)?;
                out.extend_from_slice(&chunk[..length]);
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod budget_tests {
    use super::*;
    use crate::decode_budget::with_decode_budget;
    #[test]
    fn compressed_history_is_stopped_before_growing_past_budget() {
        let mut encoded = Vec::new();
        compress(
            &mut encoded,
            &vec![b'x'; 8 * 1024 * 1024],
            CompressionType::LZ4,
        );
        assert!(encoded.len() < 64 * 1024);
        let mut out = Vec::new();
        let result = with_decode_budget(32 * 1024, || {
            decompress(&mut out, encoded.into(), CompressionType::LZ4)
        });
        assert!(result.is_err());
        assert!(out.len() <= 32 * 1024);
        with_decode_budget(4, || {
            decompress(
                &mut Vec::new(),
                Bytes::from_static(b"ab"),
                CompressionType::None,
            )
            .unwrap();
            with_decode_budget(100, || {
                decompress(
                    &mut Vec::new(),
                    Bytes::from_static(b"cd"),
                    CompressionType::None,
                )
            })
            .unwrap();
            assert!(decompress(
                &mut Vec::new(),
                Bytes::from_static(b"e"),
                CompressionType::None
            )
            .is_err());
        });
        // A failed call does not contaminate the next operation on this thread.
        with_decode_budget(1, || {
            decompress(
                &mut Vec::new(),
                Bytes::from_static(b"a"),
                CompressionType::None,
            )
        })
        .unwrap();
    }
}
