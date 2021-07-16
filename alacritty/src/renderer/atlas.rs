use crossfont::BitmapBuffer;
use log::*;

use crate::gl;
use crate::gl::types::*;

use crate::renderer::glyph::{GridAtlasGlyph, RasterizedGlyph};
use crate::renderer::math::*;
use crate::renderer::texture::*;

/// Rationale for 1024x1024 texture:
/// - for most common case (mostly ASCII-only contents and reasonable font size) this is more than
///   enough
/// - it's just 4Mb, so not a huge waste of RAM
/// Note: for less common case (larger/hidpi font, non-ASCII content) it might be advisable to make
/// it possible to increase atlas size (TODO)
static GRID_ATLAS_SIZE: i32 = 1024;

/// Error that can happen when inserting a texture to the Atlas.
#[derive(Debug)]
pub enum AtlasInsertError {
    /// Texture atlas is full.
    Full,

    /// The glyph cannot fit within a single texture.
    GlyphTooLarge,
}

/// Grid atlas entry dimensions.
pub struct CellDims {
    /// Entire cell size.
    pub size: Vec2<i32>,
}

/// Atlas to store glyphs for grid-based rendering.
/// Consists of a single table/grid of cells with the same size. Each cell can hold just one glyph.
/// Each cell can be referenced using just a pair of integer x and y coordinates.
/// Rasterized glyphs sizes and offsets are "consumed" by placing it accordingly into the atlas
/// cell.
#[derive(Debug)]
pub struct GridAtlas {
    /// OpenGL texture name/id.
    pub tex: GLuint,

    /// This atlas index/id.
    index: usize,

    /// Atlas entry size.
    cell_size: Vec2<i32>,

    /// Atlas table size in cells
    grid_size: Vec2<i32>,

    /// Next free entry coordinates
    free_line: i32,
    free_column: i32,

    /// Blitmap that is being currently filled with glyphs.
    filling_line: Blitmap,

    /// Last uploaded column in current line.
    committed_column: i32,
}

impl GridAtlas {
    /// Create new grid atlas.
    ///
    /// cell_size is the entire precomputed cell size for each element (atlas will also apply
    /// additional padding, see GRID_ATLAS_PAD_PCT) cell_offset is the position of glyph origin
    /// relative to cell left-bottom corner.
    pub fn new(index: usize, cell_size: Vec2<i32>) -> Self {
        let grid_size = (Vec2::from(GRID_ATLAS_SIZE) / cell_size).min(Vec2::from(256));

        let ret = Self {
            index,
            tex: unsafe { create_texture(GRID_ATLAS_SIZE, GRID_ATLAS_SIZE, PixelFormat::RGBA8) },
            cell_size,
            grid_size,
            free_line: 0,
            free_column: 1, // FIXME do not use sentinel 0,0 value as empty, prefere flags instead
            filling_line: Blitmap::new(GRID_ATLAS_SIZE, cell_size.y),
            committed_column: 0,
        };
        ret
    }

    /// Return atlas entry cell dimensions
    pub fn cell_dims(&self) -> CellDims {
        CellDims { size: self.cell_size }
    }

    /// Attempt to insert a new rasterized glyph into this atlas
    /// Glyphs which have offsets and sizes that make them not fit into cell dimensions will return
    /// GlyphTooLarge error.
    pub fn insert(&mut self, glyph: &RasterizedGlyph) -> Result<GridAtlasGlyph, AtlasInsertError> {
        if self.free_line >= self.grid_size.y {
            return Err(AtlasInsertError::Full);
        }

        let glyph = &glyph.rasterized;
        let line = self.free_line;
        let column = self.free_column;

        // FIXME cut rasterized glyph into cells

        let off_x = glyph.left;
        let off_y = self.cell_size.y - glyph.top;

        if off_x < 0
            || off_y < 0
            || off_x + glyph.width > self.cell_size.x
            || off_y + glyph.height > self.cell_size.y
        {
            debug!(
                "glyph '{}' {},{} {}x{} doesn't fit into atlas {}, {} cell size={:?}",
                glyph.c,
                glyph.left,
                glyph.top,
                glyph.width,
                glyph.height,
                off_x,
                off_y,
                self.cell_size,
            );

            // return Err(AtlasInsertError::GlyphTooLarge);
        }

        let off_x = std::cmp::max(0, off_x);
        let off_y = std::cmp::max(0, off_y);
        let tex_x = off_x + column * self.cell_size.x;
        let tex_y = off_y + line * self.cell_size.y;

        let colored = match &glyph.buf {
            BitmapBuffer::RGB(_) => false,
            BitmapBuffer::RGBA(_) => true,
        };

        trace!(
            "'{}' {},{} {}x{} {},{} => l={} c={} {},{}",
            glyph.c,
            glyph.left,
            glyph.top,
            glyph.width,
            glyph.height,
            off_x,
            off_y,
            line,
            column,
            tex_x,
            tex_y,
        );

        self.filling_line.blit(tex_x, off_y, self.cell_size.x, self.cell_size.y, glyph);

        self.free_column += 1;
        if self.free_column == self.grid_size.x {
            self.bind_and_commit();
            self.committed_column = 0;
            self.filling_line.clear();
            self.free_column = 0;
            self.free_line += 1;
        }

        let line = line as u16;
        let column = column as u16;
        Ok(GridAtlasGlyph { atlas_index: self.index, colored, line, column })
    }

    /// Upload cached data into OpenGL texture
    pub fn bind_and_commit(&mut self) {
        unsafe {
            gl::BindTexture(gl::TEXTURE_2D, self.tex);
        }

        // Skip if there's no new data
        if self.free_column == self.committed_column {
            return;
        }

        unsafe {
            gl::TexSubImage2D(
                gl::TEXTURE_2D,
                0,
                0, // Upload the entire line from start, TODO: optimize
                self.free_line * self.cell_size.y, // Upload the active line
                GRID_ATLAS_SIZE, // Upload  the entire line, TODO: upload only what's been written
                self.cell_size.y, // Upload only one line
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                self.filling_line.pixels.as_ptr() as *const _,
            );
        }

        self.committed_column = self.free_column;
    }
}

impl Drop for GridAtlas {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteTextures(1, &self.tex);
        }
    }
}

/// Helper struct to construct preliminary 32-bit (RGBA8) to be uploaded to a texture later.
#[derive(Debug)]
struct Blitmap {
    width: i32,
    height: i32,

    /// All pixels are 32-bit RGBA8.
    pixels: Vec<u8>,
}

impl Blitmap {
    fn new(width: i32, height: i32) -> Self {
        Self { width, height, pixels: vec![0u8; (width * height * 4) as usize] }
    }

    fn blit(
        &mut self,
        pos_x: i32,
        pos_y: i32,
        width: i32,
        height: i32,
        glyph: &crossfont::RasterizedGlyph,
    ) {
        let width = std::cmp::min(width, glyph.width);
        let height = std::cmp::min(height, glyph.height);
        match glyph.buf {
            BitmapBuffer::RGB(ref rgb) => {
                for y in 0..height {
                    for x in 0..width {
                        let dst_off = 4 * (x + pos_x + (y + pos_y) * self.width) as usize;
                        let src_off = 3 * (x + y * glyph.width) as usize;
                        self.pixels[dst_off] = rgb[src_off];
                        self.pixels[dst_off + 1] = rgb[src_off + 1];
                        self.pixels[dst_off + 2] = rgb[src_off + 2];
                        self.pixels[dst_off + 3] = 0;
                    }
                }
            }
            BitmapBuffer::RGBA(ref rgba) => {
                // let line_width = glyph.width as usize * 4;
                for y in 0..height {
                    for x in 0..width {
                        let dst_off = 4 * (x + pos_x + (y + pos_y) * self.width) as usize;
                        let src_off = 4 * (x + y * glyph.width) as usize;
                        self.pixels[dst_off] = rgba[src_off];
                        self.pixels[dst_off + 1] = rgba[src_off + 1];
                        self.pixels[dst_off + 2] = rgba[src_off + 2];
                        self.pixels[dst_off + 3] = rgba[src_off + 3];
                    }
                }
            }
        }
    }

    fn clear(&mut self) {
        self.pixels.iter_mut().for_each(|x| *x = 0);
    }
}
