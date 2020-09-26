use log::*;
use std::ptr;

use crate::gl;
use crate::gl::types::*;
use crossfont::BitmapBuffer;

use super::glyph::{AtlasRef, AtlasRefFree, AtlasRefGrid, Glyph, RasterizedGlyph};
use super::math::*;
use super::texture::*;

// TODO figure out dynamically based on GL caps (?)
static GRID_ATLAS_SIZE: i32 = 1024;
static GRID_ATLAS_PAD_PCT: Vec2<i32> = Vec2 { x: 10, y: 10 };

/// Error that can happen when inserting a texture to the Atlas.
#[derive(Debug)]
pub enum AtlasInsertError {
    /// Texture atlas is full.
    Full,

    /// The glyph cannot fit within a single texture.
    GlyphTooLarge,
}

pub struct CellDims {
    pub offset: Vec2<i32>,
    pub size: Vec2<i32>,
}

#[derive(Debug)]
pub struct GridAtlas {
    pub tex: GLuint,
    index: usize,
    cell_size: Vec2<i32>,
    cell_offset: Vec2<i32>,
    grid_size: Vec2<i32>,
    half_padding: Vec2<i32>,
    free_line: i32,
    free_column: i32,
}

impl GridAtlas {
    pub fn new(index: usize, cell_size: Vec2<i32>, cell_offset: Vec2<i32>) -> Self {
        // FIXME limit atlas size by 256x256 cells

        // FIXME add guard padding back
        let atlas_cell_size = cell_size + cell_offset;
        let padding = (atlas_cell_size * GRID_ATLAS_PAD_PCT + 99) / 100;
        let half_padding = padding / 2;
        let cell_offset = cell_offset + half_padding;
        let atlas_cell_size = atlas_cell_size + padding;

        debug!("atlas padding: {:?}", padding);

        let ret = Self {
            index,
            tex: unsafe { create_texture(GRID_ATLAS_SIZE, GRID_ATLAS_SIZE, PixelFormat::RGBA8) },
            cell_size: atlas_cell_size,
            cell_offset,
            half_padding,
            grid_size: Vec2::from(GRID_ATLAS_SIZE) / atlas_cell_size,
            free_line: 0,
            free_column: 1,
        };
        debug!("atlas: {:?}", ret);
        ret
    }

    pub fn cell_dims(&self) -> CellDims {
        CellDims { offset: self.cell_offset, size: self.cell_size }
    }

    pub fn insert(&mut self, rasterized: &RasterizedGlyph) -> Result<Glyph, AtlasInsertError> {
        if self.free_line >= self.grid_size.y {
            return Err(AtlasInsertError::Full);
        }

        let rasterized = &rasterized.rasterized;
        let line = self.free_line;
        let column = self.free_column;

        /* Atlas cell metrics in logical glyph space
         *   .----------------.<-- single glyph cell in atlas texture (self.cell_size)
         *   |                |
         *   |    .------.<---+---- rasterized glyph bbox (width, height)
         *   |    |  ##  |    |^
         *   |  . | #  # | .<-++--- (dotted box) monospace grid cell directly mapped on screen w/o overlap (not really used in atlas explicitly)
         *   |  . |#    #| .  ||
         *   |  . |######| .  ||--- rasterized.top, relative to baseline/origin.y
         *   |  . |#    #| .  ||
         *   |  . |#    #| .  ||
         *   |  . '------'-.  |v
         *   |  . . . . . . --+--- baseline
         *   |  ^             |
         *   |  |             |
         *   '--+-------------'
         *   ^  |
         *   |  `-logical monospace grid cell origin, (0, 0)
         *   `- atlas cell origin, -self.cell_offset relative to origin
         *
         * THIS BEAUTY NOW NEEDS TO BE MAPPED TO INVERSE OPENGL TEXTURE SPACE:
         *
         *   .----------------.-------
         *   |                |^   ^
         *   |  . . . . . .   ||---+-- self.cell_size.y
         *   |  . .------.-.--++---|
         *   |  . |#    #| .  || ^ |
         *   |  . |#    #| .  || | |
         *   |  . |######| .  || |-+--- rasterized.height
         *   |  . |#    #| .  || | |
         *   |  . | #  # | .  || | |-- rasterized.top
         *   |    |  ##  |    || v v
         *   |    '------'----|+-----.
         *   |                |v      } offset.y = self.cell_size.y - rasterized.top
         *   '----------------'------`
         *   ^
         *   `- atlas cell texture origin (0, 0)
         *
         */

        let off_x = self.cell_offset.x + rasterized.left;
        let off_y = self.cell_size.y - rasterized.top - self.half_padding.y;

        let tex_x = off_x + column * self.cell_size.x;
        let tex_y = off_y + line * self.cell_size.y;

        if off_x < 0
            || off_y < 0
            || off_x + rasterized.width > self.cell_size.x
            || off_y + rasterized.height > self.cell_size.y
        {
            trace!(
                "FIXME: glyph '{}' {},{} {}x{} doesn't fit into atlas cell size={:?} offset={:?}",
                rasterized.c,
                rasterized.left,
                rasterized.top,
                rasterized.width,
                rasterized.height,
                self.cell_size,
                self.cell_offset,
            );

            return Err(AtlasInsertError::GlyphTooLarge);
        }

        // FIXME don't do this:
        let wide = rasterized.width > self.cell_size.x * 3 / 2;
        let colored;

        unsafe {
            gl::BindTexture(gl::TEXTURE_2D, self.tex);

            // Load data into OpenGL.
            let (format, buf) = match &rasterized.buf {
                BitmapBuffer::RGB(buf) => {
                    colored = false;
                    (gl::RGB, buf)
                }
                BitmapBuffer::RGBA(buf) => {
                    colored = true;
                    (gl::RGBA, buf)
                }
            };

            // TODO optimize
            // 1. only copy into internal storage
            // 2. upload once before drawing by column/line subrect
            gl::TexSubImage2D(
                gl::TEXTURE_2D,
                0,
                std::cmp::max(0, tex_x), // FIXME
                std::cmp::max(0, tex_y), // FIXME
                rasterized.width,        // FIXME limit width with stride
                //std::cmp::min(rasterized.width, self.cell_size.x),
                std::cmp::min(rasterized.height, self.cell_size.y),
                format,
                gl::UNSIGNED_BYTE,
                buf.as_ptr() as *const _,
            );

            gl::BindTexture(gl::TEXTURE_2D, 0);
        }

        debug!(
            "'{}' {},{} {}x{} {},{} => l={} c={} {},{}",
            rasterized.c,
            rasterized.left,
            rasterized.top,
            rasterized.width,
            rasterized.height,
            off_x,
            off_y,
            line,
            column,
            tex_x,
            tex_y,
        );

        self.free_column += if wide { 2 } else { 1 };
        if self.free_column == self.grid_size.x {
            self.free_column = 0;
            self.free_line += 1;
        }

        let line = line as u16;
        let column = column as u16;
        Ok(Glyph {
            atlas_index: self.index,
            colored,
            atlas_ref: AtlasRef::Grid(AtlasRefGrid { line, column }),
        })
    }
}

impl Drop for GridAtlas {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteTextures(1, &self.tex);
        }
    }
}

/// Manages a single texture atlas.
///
/// The strategy for filling an atlas looks roughly like this:
///
/// ```text
///                           (width, height)
///   ┌─────┬─────┬─────┬─────┬─────┐
///   │ 10  │     │     │     │     │ <- Empty spaces; can be filled while
///   │     │     │     │     │     │    glyph_height < height - row_baseline
///   ├─────┼─────┼─────┼─────┼─────┤
///   │ 5   │ 6   │ 7   │ 8   │ 9   │
///   │     │     │     │     │     │
///   ├─────┼─────┼─────┼─────┴─────┤ <- Row height is tallest glyph in row; this is
///   │ 1   │ 2   │ 3   │ 4         │    used as the baseline for the following row.
///   │     │     │     │           │ <- Row considered full when next glyph doesn't
///   └─────┴─────┴─────┴───────────┘    fit in the row.
/// (0, 0)  x->
/// ```
#[derive(Debug)]
pub struct Atlas {
    /// Texture id for this atlas.
    pub id: GLuint,

    /// This atlas index
    index: usize,

    /// Width of atlas.
    width: i32,

    /// Height of atlas.
    height: i32,

    /// Left-most free pixel in a row.
    ///
    /// This is called the extent because it is the upper bound of used pixels
    /// in a row.
    row_extent: i32,

    /// Baseline for glyphs in the current row.
    row_baseline: i32,

    /// Tallest glyph in current row.
    ///
    /// This is used as the advance when end of row is reached.
    row_tallest: i32,
}

impl Atlas {
    pub fn new(index: usize, size: i32) -> Self {
        let mut id: GLuint = 0;
        unsafe {
            gl::PixelStorei(gl::UNPACK_ALIGNMENT, 1);
            gl::GenTextures(1, &mut id);
            gl::BindTexture(gl::TEXTURE_2D, id);
            // Use RGBA texture for both normal and emoji glyphs, since it has no performance
            // impact.
            gl::TexImage2D(
                gl::TEXTURE_2D,
                0,
                gl::RGBA as i32,
                size,
                size,
                0,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                ptr::null(),
            );

            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);

            gl::BindTexture(gl::TEXTURE_2D, 0);
        }

        Self {
            id,
            index,
            width: size,
            height: size,
            row_extent: 0,
            row_baseline: 0,
            row_tallest: 0,
        }
    }

    pub fn clear(&mut self) {
        self.row_extent = 0;
        self.row_baseline = 0;
        self.row_tallest = 0;
    }

    /// Insert a RasterizedGlyph into the texture atlas.
    pub fn insert(&mut self, glyph: &RasterizedGlyph) -> Result<Glyph, AtlasInsertError> {
        let glyph = &glyph.rasterized;
        if glyph.width > self.width || glyph.height > self.height {
            return Err(AtlasInsertError::GlyphTooLarge);
        }

        // If there's not enough room in current row, go onto next one.
        if !self.room_in_row(glyph) {
            self.advance_row()?;
        }

        // If there's still not room, there's nothing that can be done here..
        if !self.room_in_row(glyph) {
            return Err(AtlasInsertError::Full);
        }

        // There appears to be room; load the glyph.
        Ok(self.insert_inner(glyph))
    }

    /// Insert the glyph without checking for room.
    ///
    /// Internal function for use once atlas has been checked for space. GL
    /// errors could still occur at this point if we were checking for them;
    /// hence, the Result.
    fn insert_inner(&mut self, glyph: &crossfont::RasterizedGlyph) -> Glyph {
        let offset_y = self.row_baseline;
        let offset_x = self.row_extent;
        let height = glyph.height as i32;
        let width = glyph.width as i32;
        let colored;

        unsafe {
            gl::BindTexture(gl::TEXTURE_2D, self.id);

            // Load data into OpenGL.
            let (format, buf) = match &glyph.buf {
                BitmapBuffer::RGB(buf) => {
                    colored = false;
                    (gl::RGB, buf)
                }
                BitmapBuffer::RGBA(buf) => {
                    colored = true;
                    (gl::RGBA, buf)
                }
            };

            gl::TexSubImage2D(
                gl::TEXTURE_2D,
                0,
                offset_x,
                offset_y,
                width,
                height,
                format,
                gl::UNSIGNED_BYTE,
                buf.as_ptr() as *const _,
            );
        }

        // Update Atlas state.
        self.row_extent = offset_x + width;
        if height > self.row_tallest {
            self.row_tallest = height;
        }

        // Generate UV coordinates.
        let uv_bot = offset_y as f32 / self.height as f32;
        let uv_left = offset_x as f32 / self.width as f32;
        let uv_height = height as f32 / self.height as f32;
        let uv_width = width as f32 / self.width as f32;

        Glyph {
            atlas_index: self.index,
            colored,
            atlas_ref: AtlasRef::Free(AtlasRefFree {
                top: glyph.top as i16,
                width: width as i16,
                height: height as i16,
                left: glyph.left as i16,
                uv_bot,
                uv_left,
                uv_width,
                uv_height,
            }),
        }
    }

    /// Check if there's room in the current row for given glyph.
    fn room_in_row(&self, raw: &crossfont::RasterizedGlyph) -> bool {
        let next_extent = self.row_extent + raw.width as i32;
        let enough_width = next_extent <= self.width;
        let enough_height = (raw.height as i32) < (self.height - self.row_baseline);

        enough_width && enough_height
    }

    /// Mark current row as finished and prepare to insert into the next row.
    fn advance_row(&mut self) -> Result<(), AtlasInsertError> {
        let advance_to = self.row_baseline + self.row_tallest;
        if self.height - advance_to <= 0 {
            return Err(AtlasInsertError::Full);
        }

        self.row_baseline = advance_to;
        self.row_extent = 0;
        self.row_tallest = 0;

        Ok(())
    }
}
