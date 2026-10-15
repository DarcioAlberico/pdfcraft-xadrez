//! Raster inputs owned by this crate: an 8-bit gray image whose size always matches its data.
//!
//! The engine (L6) fills these from whatever it rendered or decoded; this crate never decodes
//! image files itself.

/// Most pixels a [`GrayImage`] may hold: 100 megapixels (10 000 × 10 000), about 33 × 33 inches
/// at 300 DPI. A safety cap on the work the detectors do per image, not a measurement: a larger
/// raster is refused rather than scanned.
pub const MAX_GRAY_PIXELS: usize = 100_000_000;

/// Why a [`GrayImage`] could not be built.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RasterError {
    #[error("an image needs at least one pixel in each direction, got {width} by {height}")]
    Empty { width: u32, height: u32 },
    #[error("{width} by {height} pixels is more than the {max} a gray image may hold")]
    TooLarge { width: u32, height: u32, max: usize },
    #[error("{width} by {height} pixels needs {expected} bytes of data, got {actual}")]
    LengthMismatch { width: u32, height: u32, expected: usize, actual: usize },
}

/// An 8-bit gray raster, row-major, top row first, one byte per pixel (0 is black).
///
/// Built only through [`GrayImage::new`], which checks that `data.len() == width * height`, so
/// every pixel lookup inside the bounds is valid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrayImage {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl GrayImage {
    /// A gray image of `width` × `height` pixels from row-major `data`.
    pub fn new(width: u32, height: u32, data: Vec<u8>) -> Result<Self, RasterError> {
        if width == 0 || height == 0 {
            return Err(RasterError::Empty { width, height });
        }
        let too_large = RasterError::TooLarge { width, height, max: MAX_GRAY_PIXELS };
        let expected = usize::try_from(u64::from(width) * u64::from(height)).map_err(|_| too_large.clone())?;
        if expected > MAX_GRAY_PIXELS {
            return Err(too_large);
        }
        if data.len() != expected {
            return Err(RasterError::LengthMismatch { width, height, expected, actual: data.len() });
        }
        Ok(Self { width, height, data })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// The pixels, row-major, top row first.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// The pixel at column `x`, row `y`, or `None` outside the image.
    pub fn get(&self, x: u32, y: u32) -> Option<u8> {
        if x >= self.width || y >= self.height {
            return None;
        }
        // Both are inside the image, whose pixel count fits a usize (checked in `new`).
        let index = usize::try_from(u64::from(y) * u64::from(self.width) + u64::from(x)).ok()?;
        self.data.get(index).copied()
    }

    /// Row `y`, or `None` outside the image.
    pub fn row(&self, y: u32) -> Option<&[u8]> {
        if y >= self.height {
            return None;
        }
        let width = usize::try_from(self.width).ok()?;
        let start = usize::try_from(y).ok()?.checked_mul(width)?;
        self.data.get(start..start.checked_add(width)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_that_the_data_fills_the_image() {
        assert!(GrayImage::new(3, 2, vec![0; 6]).is_ok());
        assert_eq!(GrayImage::new(3, 2, vec![0; 5]), Err(RasterError::LengthMismatch { width: 3, height: 2, expected: 6, actual: 5 }));
        assert!(matches!(GrayImage::new(3, 2, vec![0; 7]), Err(RasterError::LengthMismatch { .. })));
    }

    #[test]
    fn refuses_empty_and_oversized_images() {
        assert_eq!(GrayImage::new(0, 4, Vec::new()), Err(RasterError::Empty { width: 0, height: 4 }));
        assert_eq!(GrayImage::new(4, 0, Vec::new()), Err(RasterError::Empty { width: 4, height: 0 }));
        // u32::MAX squared overflows a 32-bit usize (wasm32) and exceeds the cap on 64-bit.
        assert!(matches!(GrayImage::new(u32::MAX, u32::MAX, Vec::new()), Err(RasterError::TooLarge { .. })));
    }

    #[test]
    fn looks_pixels_up_row_major() {
        let image = GrayImage::new(3, 2, vec![1, 2, 3, 4, 5, 6]).unwrap();
        assert_eq!((image.get(0, 0), image.get(2, 0), image.get(0, 1), image.get(2, 1)), (Some(1), Some(3), Some(4), Some(6)));
        assert_eq!((image.get(3, 0), image.get(0, 2), image.get(u32::MAX, u32::MAX)), (None, None, None));
    }
}
