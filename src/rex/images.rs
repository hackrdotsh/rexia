//! Kitty graphics: the images a terminal shows, read from its render state for the UI.
//!
//! The terminal decodes the protocol and stores images; the render state's image list
//! positions every placement on screen, Unicode placeholders and relative placements
//! included. Here each placement is paired with its image's pixels, converted to RGBA
//! once per image generation and shared between frames.
use anyhow::Result;
use ghostty_vt::{
    Terminal,
    kitty::graphics::{Compression, ImageFormat},
    render::{ImageIterator, ImageLayer, Snapshot},
};
use std::{cell::Cell, collections::HashMap, sync::Arc};

/// How much image data one terminal may hold, as Ghostty's default.
pub const STORAGE_LIMIT: u64 = 320 * 1024 * 1024;

/// The character a program prints where a virtual placement shows (Unicode placeholder).
pub const PLACEHOLDER: char = '\u{10EEEE}';

/// An image's pixels, 8-bit RGBA, row by row.
pub struct Pixels {
    pub id: u32,
    pub generation: u64,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl std::fmt::Debug for Pixels {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pixels")
            .field("id", &self.id)
            .field("generation", &self.generation)
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

/// Where an image draws relative to the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    /// Under the cell backgrounds.
    BelowBackground,
    /// Over the backgrounds, under the text.
    BelowText,
    /// Over the text.
    AboveText,
}

/// One image on screen. Positions are in cells of the viewport; offsets and sizes are in
/// the terminal's pixels (`Frame::cell_px` per cell), so the UI scales them to its cells.
#[derive(Clone, Debug)]
pub struct Placement {
    pub pixels: Arc<Pixels>,
    pub layer: Layer,
    pub z: i32,
    pub col: i32,
    pub row: i32,
    pub offset_x: u32,
    pub offset_y: u32,
    pub width: u32,
    pub height: u32,
    /// The part of the image to draw: x, y, width, height in image pixels.
    pub source: [u32; 4],
}

/// Converted images, kept while they're on screen.
#[derive(Default)]
pub struct ImageCache {
    pixels: HashMap<(u32, u64), Arc<Pixels>>,
}

thread_local! {
    static PNG_DECODER: Cell<bool> = const { Cell::new(false) };
}

/// Let terminals on this thread take PNG images. The decoder is per thread, and the
/// terminal decodes on the thread that writes to it.
pub fn install_png_decoder() {
    PNG_DECODER.with(|installed| {
        if !installed.get() {
            let decoder = ghostty_vt::kitty::graphics::RustPngDecoder::new();
            if ghostty_vt::kitty::graphics::set_png_decoder(Some(Box::new(decoder))).is_ok() {
                installed.set(true);
            }
        }
    });
}

/// Turn on Kitty graphics for a terminal. Files, temporary files and shared memory name
/// things on the machine the program runs on, so they're only read when that's this one.
/// Temporary files must be in `/tmp` and have `tty-graphics-protocol` in their names, and
/// the terminal deletes them once read, as Kitty does.
pub fn enable(terminal: &mut Terminal, local: bool) -> Result<Mediums> {
    terminal.set_kitty_image_storage_limit(STORAGE_LIMIT)?;
    terminal.set_kitty_image_from_file_allowed(local)?;
    terminal.set_kitty_image_temp_file_dir(local.then_some(TEMP_DIR))?;
    terminal.set_kitty_image_from_shared_mem_allowed(local)?;
    Ok(Mediums {
        file: terminal.is_kitty_image_from_file_allowed()?,
        temp_file: !terminal.kitty_image_temp_file_dir()?.is_empty(),
        shared_memory: terminal.is_kitty_image_from_shared_mem_allowed()?,
    })
}

/// Where programs put temporary image files: `/tmp`, as Kitty's own `icat` does.
const TEMP_DIR: &str = "/tmp";

/// Which ways of sending images, besides directly, a terminal takes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mediums {
    pub file: bool,
    pub temp_file: bool,
    pub shared_memory: bool,
}

/// Stored image data as RGBA, whatever format the program sent.
fn to_rgba(format: ImageFormat, data: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let pixels = width as usize * height as usize;
    let rgba = match format {
        ImageFormat::Rgba | ImageFormat::Png => data.get(..pixels * 4)?.to_vec(),
        ImageFormat::Rgb => data
            .get(..pixels * 3)?
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        ImageFormat::GrayAlpha => data
            .get(..pixels * 2)?
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        ImageFormat::Gray => data
            .get(..pixels)?
            .iter()
            .flat_map(|&g| [g, g, g, 255])
            .collect(),
        _ => return None,
    };
    Some(rgba)
}

impl ImageCache {
    /// The placements of the snapshot's images, in drawing order: by layer, then z.
    pub fn placements(
        &mut self,
        snapshot: &Snapshot<'_>,
        iterator: &mut ImageIterator,
        terminal: &Terminal,
    ) -> Result<Vec<Placement>> {
        let mut found = Vec::new();
        let mut iteration = iterator.update(snapshot)?;
        while let Some(placement) = iteration.next() {
            found.push(placement?);
        }
        if found.is_empty() {
            self.pixels.clear();
            return Ok(Vec::new());
        }
        let graphics = terminal.kitty_graphics()?;
        let mut placements = Vec::with_capacity(found.len());
        let mut live = HashMap::new();
        for p in found {
            let key = (p.image_id, p.image_generation);
            let pixels = match self.pixels.get(&key) {
                Some(pixels) => pixels.clone(),
                None => {
                    let Some(image) = graphics.image(p.image_id) else {
                        continue;
                    };
                    let (width, height) = (image.width()?, image.height()?);
                    // The terminal inflates compressed data as it loads it.
                    if image.compression()? != Compression::None {
                        continue;
                    }
                    let Some(rgba) = to_rgba(image.format()?, image.data()?, width, height) else {
                        continue;
                    };
                    Arc::new(Pixels {
                        id: p.image_id,
                        generation: p.image_generation,
                        width,
                        height,
                        rgba,
                    })
                }
            };
            live.insert(key, pixels.clone());
            placements.push(Placement {
                pixels,
                layer: match p.layer {
                    ImageLayer::BelowBackground => Layer::BelowBackground,
                    ImageLayer::BelowText => Layer::BelowText,
                    ImageLayer::AboveText => Layer::AboveText,
                },
                z: p.z,
                col: p.col,
                row: p.row,
                offset_x: p.offset_x,
                offset_y: p.offset_y,
                width: p.pixel_width,
                height: p.pixel_height,
                source: [p.source_x, p.source_y, p.source_width, p.source_height],
            });
        }
        self.pixels = live;
        placements.sort_by_key(|p| (p.layer, p.z));
        Ok(placements)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_become_rgba() {
        assert_eq!(
            to_rgba(ImageFormat::Rgb, &[1, 2, 3, 4, 5, 6], 2, 1),
            Some(vec![1, 2, 3, 255, 4, 5, 6, 255])
        );
        assert_eq!(
            to_rgba(ImageFormat::GrayAlpha, &[9, 128], 1, 1),
            Some(vec![9, 9, 9, 128])
        );
        assert_eq!(
            to_rgba(ImageFormat::Gray, &[7], 1, 1),
            Some(vec![7, 7, 7, 255])
        );
        assert_eq!(
            to_rgba(ImageFormat::Rgba, &[1, 2, 3], 1, 1),
            None,
            "short data"
        );
    }
}
