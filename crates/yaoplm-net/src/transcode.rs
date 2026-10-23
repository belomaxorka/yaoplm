//! Any image in, an OPL-ready PNG out (ART-2, ADR-0005).
//!
//! OPL stretches whatever it loads into the theme's element
//! (`src/textures.c`, ps2homebrew/Open-PS2-Loader@b3a21997), so the stored
//! art is resized to the profile's exact size without cropping, the way
//! OPL Manager, OPL PC Tools and PyOPLM do. Logos are the exception: they
//! come in any shape, so they are fitted into the size with transparent
//! margins instead of being squashed. Lanczos3 through `fast_image_resize`;
//! 8 bits per channel, which every loader reads (RiptOPL rejects 16-bit
//! PNG); never wider or taller than 1024 pixels.

use std::io::Cursor;

use fast_image_resize::{FilterType, ResizeAlg, ResizeOptions, Resizer};
use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
use image::{DynamicImage, GenericImageView, ImageEncoder, ImageReader, Limits};
use yaoplm_formats::art::{ArtKind, ArtSize, MAX_DIMENSION, png_header};

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

/// How a picture meets its size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fit {
    /// Resized to the exact size, as OPL draws it anyway.
    #[default]
    Stretch,
    /// Scaled to fit inside the size, centred on a transparent canvas.
    Contain,
}

impl Fit {
    /// Logos keep their shape; everything else fills its element.
    pub fn for_kind(kind: ArtKind) -> Self {
        match kind {
            ArtKind::Lgo => Self::Contain,
            _ => Self::Stretch,
        }
    }
}

fn resize(source: &DynamicImage, width: u32, height: u32) -> Result<DynamicImage, NetError> {
    if source.dimensions() == (width, height) {
        return Ok(source.clone());
    }
    let mut target = match source {
        DynamicImage::ImageRgba8(_) => DynamicImage::new_rgba8(width, height),
        _ => DynamicImage::new_rgb8(width, height),
    };
    let options = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3));
    Resizer::new()
        .resize(source, &mut target, &options)
        .map_err(|e| NetError::Image(e.to_string()))?;
    Ok(target)
}

/// Resizes `bytes` (PNG, JPEG, BMP, GIF or WebP) to `size` and encodes a
/// PNG with 8 bits per channel: RGB, or RGBA when the input has alpha. A
/// PNG that already has the size, 8 bits or fewer and no interlacing is
/// kept as it is.
pub fn to_png(bytes: &[u8], size: ArtSize) -> Result<Transcoded, NetError> {
    to_png_fit(bytes, size, Fit::Stretch)
}

/// [`to_png`] with a choice of [`Fit`]; `Contain` yields RGBA unless the
/// PNG is kept as it is.
pub fn to_png_fit(bytes: &[u8], size: ArtSize, fit: Fit) -> Result<Transcoded, NetError> {
    let width = size.width.clamp(1, MAX_DIMENSION);
    let height = size.height.clamp(1, MAX_DIMENSION);
    let decoded = decode(bytes)?;
    let (source_width, source_height) = decoded.dimensions();
    // OPL Manager's backups hold palette PNGs at OPL's sizes (research/13
    // §3): kept byte for byte they load as palette textures (T4/T8, one
    // byte a pixel at most); re-encoded as RGB they take 2-2.6 times the
    // space on the device and 3-4 times the texture memory.
    if (source_width, source_height) == (width, height) && usable_as_is(bytes) {
        return Ok(Transcoded {
            png: bytes.to_vec(),
            width,
            height,
            source_width,
            source_height,
        });
    }
    let resized = match fit {
        Fit::Stretch => {
            let source = if decoded.color().has_alpha() {
                DynamicImage::ImageRgba8(decoded.into_rgba8())
            } else {
                DynamicImage::ImageRgb8(decoded.into_rgb8())
            };
            resize(&source, width, height)?
        }
        Fit::Contain => {
            let scale = f64::min(
                f64::from(width) / f64::from(source_width.max(1)),
                f64::from(height) / f64::from(source_height.max(1)),
            );
            let inner_width = ((f64::from(source_width) * scale).round() as u32).clamp(1, width);
            let inner_height = ((f64::from(source_height) * scale).round() as u32).clamp(1, height);
            let inner = resize(
                &DynamicImage::ImageRgba8(decoded.into_rgba8()),
                inner_width,
                inner_height,
            )?;
            let mut canvas = image::RgbaImage::new(width, height);
            image::imageops::overlay(
                &mut canvas,
                &inner.into_rgba8(),
                i64::from((width - inner_width) / 2),
                i64::from((height - inner_height) / 2),
            );
            DynamicImage::ImageRgba8(canvas)
        }
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

/// A PNG OPL can load as it is: 8 bits or fewer per channel or palette
/// index (RiptOPL refuses 16) and not interlaced.
fn usable_as_is(bytes: &[u8]) -> bool {
    png_header(bytes).is_some_and(|h| h.bit_depth <= 8) && bytes.get(28) == Some(&0)
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
    use std::io::Write;

    use image::{ImageBuffer, Rgb, Rgba};

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

    /// A PNG with a palette of four colours, like the pictures of OPL
    /// Manager's backups since 2024 (`image` cannot write one).
    fn palette_png(width: u32, height: u32) -> Vec<u8> {
        fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            out.extend_from_slice(kind);
            out.extend_from_slice(data);
            let mut crc = crc32fast::Hasher::new();
            crc.update(kind);
            crc.update(data);
            out.extend_from_slice(&crc.finalize().to_be_bytes());
        }
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&width.to_be_bytes());
        ihdr.extend_from_slice(&height.to_be_bytes());
        ihdr.extend_from_slice(&[8, 3, 0, 0, 0]);
        let mut rows = Vec::new();
        for y in 0..height {
            rows.push(0);
            rows.extend((0..width).map(|x| ((x + y) % 4) as u8));
        }
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(&rows).unwrap();
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        chunk(&mut out, b"IHDR", &ihdr);
        chunk(
            &mut out,
            b"PLTE",
            &[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255],
        );
        chunk(&mut out, b"IDAT", &z.finish().unwrap());
        chunk(&mut out, b"IEND", &[]);
        out
    }

    #[test]
    fn a_png_that_fits_is_kept_as_it_is() {
        let size = ArtSize {
            width: 140,
            height: 200,
        };
        let png = palette_png(140, 200);
        assert_eq!(png_header(&png).unwrap().color_type, 3, "palette");
        let out = to_png(&png, size).unwrap();
        assert_eq!(out.png, png, "byte for byte");
        // Fitted logos too, when they already have the size.
        let logo = palette_png(300, 125);
        let out = to_png_fit(
            &logo,
            ArtSize {
                width: 300,
                height: 125,
            },
            Fit::Contain,
        )
        .unwrap();
        assert_eq!(out.png, logo);
        // Another size is resized as before, to RGB.
        let out = to_png(&palette_png(280, 400), size).unwrap();
        let header = png_header(&out.png).unwrap();
        assert_eq!(
            (header.width, header.height, header.color_type),
            (140, 200, 2)
        );
        // A damaged PNG of the right size is refused, not passed on.
        let mut broken = png.clone();
        let at = broken.len() - 20;
        broken[at] ^= 0xFF;
        assert!(to_png(&broken, size).is_err());
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
        // Even at its own size: 16 bits never reach the device.
        let out = to_png(
            &rgba16,
            ArtSize {
                width: 10,
                height: 10,
            },
        )
        .unwrap();
        assert_eq!(png_header(&out.png).unwrap().bit_depth, 8);
    }

    #[test]
    fn logos_keep_their_shape() {
        let size = ArtSize {
            width: 300,
            height: 125,
        };
        // A square logo: centred, transparent at the sides.
        let out = to_png_fit(&jpeg(200, 200), size, Fit::Contain).unwrap();
        let header = png_header(&out.png).unwrap();
        assert_eq!((header.width, header.height), (300, 125));
        assert_eq!(header.color_type, 6, "RGBA, for the margins");
        let img = image::load_from_memory(&out.png).unwrap().into_rgba8();
        assert_eq!(img.get_pixel(0, 60)[3], 0, "left margin");
        assert_eq!(img.get_pixel(299, 60)[3], 0, "right margin");
        assert_eq!(img.get_pixel(150, 60)[3], 255, "the logo itself");
        assert_eq!(Fit::for_kind(ArtKind::Lgo), Fit::Contain);
        assert_eq!(Fit::for_kind(ArtKind::Scr), Fit::Stretch);
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
