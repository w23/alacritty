use crate::gl;
use crate::gl::types::*;
use alacritty_terminal::term;

use super::Glyph;

use crossfont::{BitmapBuffer, RasterizedGlyph};

use super::texture::*;

use log::*;

// TODO figure out dynamically based on GL caps
static GRID_ATLAS_SIZE: i32 = 1024;
static GRID_ATLAS_PAD_PCT_X: i32 = 50;
static GRID_ATLAS_PAD_PCT_Y: i32 = 25;

#[derive(Debug)]
pub enum AtlasError {
    TooBig { w: i32, h: i32, cw: i32, ch: i32 },
    Full,
}

pub struct CellDims {
    pub off_x: i32,
    pub off_y: i32,
    pub size_x: i32,
    pub size_y: i32,
}

#[derive(Debug)]
pub struct GridAtlas {
    pub tex: GLuint,
    cell_width: i32,
    cell_height: i32,
    cell_offset_x: i32,
    cell_offset_y: i32,
    grid_width: i32,
    grid_height: i32,
    free_line: i32,
    free_column: i32,
}

impl GridAtlas {
    pub fn new(size_info: &term::SizeInfo) -> Self {
        // FIXME limit atlas size by 256x256 cells
        let cell_width = (size_info.cell_width as i32) * (100 + GRID_ATLAS_PAD_PCT_X) / 100;
        let cell_height = (size_info.cell_height as i32) * (100 + GRID_ATLAS_PAD_PCT_Y) / 100;
        let ret = Self {
            tex: unsafe { create_texture(GRID_ATLAS_SIZE, GRID_ATLAS_SIZE, PixelFormat::RGBA8) },
            grid_width: GRID_ATLAS_SIZE / cell_width,
            grid_height: GRID_ATLAS_SIZE / cell_height,
            cell_offset_x: (cell_width - (size_info.cell_width as i32)) / 2,
            cell_offset_y: (cell_height - (size_info.cell_height as i32)) / 2,
            cell_width,
            cell_height,
            free_line: 0,
            free_column: 1,
        };
        debug!("atlas: {:#?}", ret);
        ret
    }

    pub fn cell_dims(&self) -> CellDims {
        CellDims {
            off_x: self.cell_offset_x,
            off_y: self.cell_offset_y,
            size_x: self.cell_width,
            size_y: self.cell_height,
        }
    }

    pub fn load_glyph(&mut self, rasterized: &RasterizedGlyph) -> Result<Glyph, AtlasError> {
        if rasterized.width + self.cell_offset_x > self.cell_width
            || rasterized.height + self.cell_offset_y > self.cell_height
        {
            error!(
                "FIXME: glyph '{}' {},{} {}x{} doesn't fit into atlas cell",
                rasterized.c, rasterized.left, rasterized.top, rasterized.width, rasterized.height,
            );

            // return Err(AtlasError::TooBig {
            //     w: rasterized.width,
            //     h: rasterized.height,
            //     cw: self.cell_width,
            //     ch: self.cell_height,
            // });
        }

        if self.free_line >= self.cell_height {
            return Err(AtlasError::Full);
        }

        // FIXME don't do this:
        let wide = rasterized.width > self.cell_width * 3 / 2;

        let colored;
        let line = self.free_line;
        let column = self.free_column;

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
            let off_x = self.cell_offset_x + rasterized.left;
            let tex_x = off_x + column * self.cell_width;
            let off_y = -self.cell_offset_y + self.cell_height - rasterized.top; //+ rasterized.height; // - rasterized.top;
            let tex_y = off_y + line * self.cell_height;
            gl::TexSubImage2D(
                gl::TEXTURE_2D,
                0,
                std::cmp::max(0, tex_x),
                std::cmp::max(0, tex_y),
                rasterized.width,
                rasterized.height,
                format,
                gl::UNSIGNED_BYTE,
                buf.as_ptr() as *const _,
            );

            gl::BindTexture(gl::TEXTURE_2D, 0);

            debug!(
                "{} {},{} {}x{} {},{} => l={} c={} {},{}",
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
        }

        self.free_column += if wide { 2 } else { 1 };
        if self.free_column == self.grid_width {
            self.free_column = 0;
            self.free_line += 1;
        }

        // TODO make Glyph enum
        Ok(Glyph {
            tex_id: self.tex,
            colored,
            top: 0.0,
            left: 0.0,
            width: 0.0,
            height: 0.0,
            uv_bot: line as f32,
            uv_left: column as f32,
            uv_width: 0.0,
            uv_height: 0.0,
        })
    }
}

impl Drop for GridAtlas {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteTextures(1, &mut self.tex);
        }
    }
}
