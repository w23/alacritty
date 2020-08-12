use crate::gl;
use crate::gl::types::*;
use alacritty_terminal::term;

use super::Glyph;

use crossfont::{BitmapBuffer, RasterizedGlyph};

use super::texture::*;

use log::*;

// TODO figure out dynamically based on GL caps
static GRID_ATLAS_SIZE: i32 = 1024;

#[derive(Debug)]
pub enum AtlasError {
    TooBig { w: i32, h: i32, cw: i32, ch: i32 },
    Full,
}

#[derive(Debug)]
pub struct GridAtlas {
    pub tex: GLuint,
    cell_width: i32,
    cell_height: i32,
    grid_width: i32,
    grid_height: i32,
    free_line: i32,
    free_column: i32,
}

impl GridAtlas {
    pub fn new(props: &term::SizeInfo) -> Self {
        // FIXME limit atlas size by 256x256 cells
        let cell_width = props.cell_width as i32;
        let cell_height = props.cell_height as i32;
        Self {
            tex: unsafe { create_texture(GRID_ATLAS_SIZE, GRID_ATLAS_SIZE, PixelFormat::RGBA8) },
            grid_width: GRID_ATLAS_SIZE / cell_width,
            grid_height: GRID_ATLAS_SIZE / cell_height,
            cell_width,
            cell_height,
            free_line: 0,
            free_column: 1,
        }
    }

    pub fn load_glyph(&mut self, rasterized: &RasterizedGlyph) -> Result<Glyph, AtlasError> {
        if rasterized.width > self.cell_width || rasterized.height > self.cell_height {
            debug!(
                "{} {},{} {}x{}",
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
            gl::TexSubImage2D(
                gl::TEXTURE_2D,
                0,
                std::cmp::max(0, column * self.cell_width + rasterized.left),
                std::cmp::max(0, line * self.cell_height + self.cell_height - rasterized.top),
                rasterized.width,
                rasterized.height,
                format,
                gl::UNSIGNED_BYTE,
                buf.as_ptr() as *const _,
            );

            gl::BindTexture(gl::TEXTURE_2D, 0);
        }

        // eprintln!(
        //     "{} {},{} {}x{} => l={} c={}",
        //     rasterized.c,
        //     rasterized.left,
        //     rasterized.top,
        //     rasterized.width,
        //     rasterized.height,
        //     line,
        //     column
        // );

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
