//! Any image in, an OPL-ready PNG out (ART-2, ADR-0005).
//!
//! OPL stretches whatever it loads into the theme's element
//! (`src/textures.c`, ps2homebrew/Open-PS2-Loader@b3a21997), so the stored
//! art is resized to the profile's exact size without cropping, the way
//! OPL Manager, OPL PC Tools and PyOPLM do. Lanczos3 through
//! `fast_image_resize`; 8 bits per channel, which every loader reads
//! (RiptOPL rejects 16-bit PNG); never wider or taller than 1024 pixels.

use std::io::Cursor;

use fast_image_resize::{FilterType, ResizeAlg, ResizeOptions, Resizer};
use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
use image::{DynamicImage, GenericImageView, ImageEncoder, ImageReader, Limits};
use yaoplm_formats::art::{ArtSize, MAX_DIMENSION};

use crate::NetError;

/// Decoding limits: a cover is a few megapixels at most.
const MAX_SOURCE_DIMENSION: u32 = 8192;
const MAX_DECODE_BYTES: u64 = 256 << 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcoded {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// The decoded input, before resizing.
    pub source_width: u32,
    pub source_height: u32,
}

fn decode(bytes: &[u8]) -> Result<DynamicImage, NetError> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| NetError::Image(e.to_string()))?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SOURCE_DIMENSION);
    limits.max_image_height = Some(MAX_SOURCE_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);
    if reader.format().is_none() {
        return Err(NetError::Image("not an image".into()));
    }
    reader.decode().map_err(|e| NetError::Image(e.to_string()))
}

/// Resizes `bytes` (PNG, JPEG, BMP, GIF or WebP) to `size` and encodes a
/// PNG with 8 bits per channel: RGB, or RGBA when the input has alpha.
pub fn to_png(bytes: &[u8], size: ArtSize) -> Result<Transcoded, NetError> {
    let width = size.width.clamp(1, MAX_DIMENSION);
    let height = size.height.clamp(1, MAX_DIMENSION);
    let decoded = decode(bytes)?;
    let (source_width, source_height) = decoded.dimensions();
    let source = if decoded.color().has_alpha() {
        DynamicImage::ImageRgba8(decoded.into_rgba8())
    } else {
        DynamicImage::ImageRgb8(decoded.into_rgb8())
    };
    let resized = if (source_width, source_height) == (width, height) {
        source
    } else {
        let mut target = match &source {
            DynamicImage::ImageRgba8(_) => DynamicImage::new_rgba8(width, height),
            _ => DynamicImage::new_rgb8(width, height),
        };
        let options = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3));
        Resizer::new()
            .resize(&source, &mut target, &options)
            .map_err(|e| NetError::Image(e.to_string()))?;
        target
    };
    let mut png = Vec::new();
    PngEncoder::new_with_quality(&mut png, CompressionType::Default, PngFilter::Adaptive)
        .write_image(resized.as_bytes(), width, height, resized.color().into())
        .map_err(|e| NetError::Image(e.to_string()))?;
    Ok(Transcoded {
        png,
        width,
        height,
        source_width,
        source_height,
    })
}

/// True when `bytes` start like an image we can decode.
pub fn looks_like_image(bytes: &[u8]) -> bool {
    ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()
        .and_then(|r| r.format())
        .is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb, Rgba};
    use yaoplm_formats::art::png_header;

    fn jpeg(width: u32, height: u32) -> Vec<u8> {
        let img = ImageBuffer::from_fn(width, height, |x, y| {
            Rgb([(x % 256) as u8, (y % 256) as u8, 128])
        });
        let mut out = Vec::new();
        DynamicImage::ImageRgb8(img)
            .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Jpeg)
            .unwrap();
        out
    }

    #[test]
    fn jpeg_becomes_png_of_the_profile_size() {
        let size = ArtSize {
            width: 140,
            height: 200,
        };
        let out = to_png(&jpeg(512, 731), size).unwrap();
        let header = png_header(&out.png).unwrap();
        assert_eq!((header.width, header.height), (140, 200));
        assert_eq!(header.bit_depth, 8);
        assert_eq!(header.color_type, 2, "RGB without alpha");
        assert_eq!((out.source_width, out.source_height), (512, 731));
        // Upscaling works too, and an exact match is passed through.
        let out = to_png(&jpeg(70, 100), size).unwrap();
        assert_eq!(png_header(&out.png).unwrap().width, 140);
        let out = to_png(&jpeg(140, 200), size).unwrap();
        assert_eq!(png_header(&out.png).unwrap().height, 200);
    }

    #[test]
    fn alpha_is_kept_and_16_bit_is_flattened() {
        let img = ImageBuffer::from_fn(10, 10, |x, _| Rgba([x as u8, 0, 0, 128]));
        let mut rgba16 = Vec::new();
        DynamicImage::ImageRgba8(img)
            .into_rgba16()
            .write_to(&mut Cursor::new(&mut rgba16), image::ImageFormat::Png)
            .unwrap();
        assert_eq!(png_header(&rgba16).unwrap().bit_depth, 16);
        let out = to_png(
            &rgba16,
            ArtSize {
                width: 64,
                height: 64,
            },
        )
        .unwrap();
        let header = png_header(&out.png).unwrap();
        assert_eq!(header.bit_depth, 8);
        assert_eq!(header.color_type, 6, "RGBA");
    }

    #[test]
    fn garbage_and_oversized_targets_are_refused() {
        let size = ArtSize {
            width: 140,
            height: 200,
        };
        assert!(matches!(
            to_png(b"not an image", size),
            Err(NetError::Image(_))
        ));
        assert!(matches!(
            to_png(&[0x89, b'P', b'N', b'G'], size),
            Err(NetError::Image(_))
        ));
        assert!(!looks_like_image(b"hello"));
        assert!(looks_like_image(&jpeg(4, 4)));
        // Targets are clamped to OPL's limit (ADR-0005).
        let out = to_png(
            &jpeg(8, 8),
            ArtSize {
                width: 5000,
                height: 10,
            },
        )
        .unwrap();
        assert_eq!(out.width, MAX_DIMENSION);
    }
}
