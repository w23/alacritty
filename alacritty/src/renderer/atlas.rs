use crate::gl;
use crate::gl::types::*;
use crossfont::{BitmapBuffer, RasterizedGlyph};

use super::texture::*;
use super::Glyph;

use log::*;

// TODO figure out dynamically based on GL caps
static GRID_ATLAS_SIZE: i32 = 1024;
static GRID_ATLAS_PAD_PCT: Vec2<i32> = Vec2 { x: 0, y: 0 };

#[derive(Debug, Copy, Clone)]
pub struct Vec2<T: Copy> {
    pub x: T,
    pub y: T,
}

impl<T: Copy> Vec2<T> {
    pub fn new(x: T, y: T) -> Self {
        Self { x, y }
    }
}

impl<T: std::ops::Add<Output = T> + Copy> std::ops::Add for Vec2<T> {
    type Output = Vec2<T>;

    fn add(self, rhs: Self) -> Self {
        Self { x: self.x + rhs.x, y: self.y + rhs.y }
    }
}

impl<T: std::ops::Add<Output = T> + Copy> std::ops::Add<T> for Vec2<T> {
    type Output = Vec2<T>;

    fn add(self, rhs: T) -> Self {
        Self { x: self.x + rhs, y: self.y + rhs }
    }
}

impl<T: std::ops::Sub<Output = T> + Copy> std::ops::Sub for Vec2<T> {
    type Output = Vec2<T>;

    fn sub(self, rhs: Self) -> Self {
        Self { x: self.x - rhs.x, y: self.y - rhs.y }
    }
}

impl<T: std::ops::Sub<Output = T> + Copy> std::ops::Sub<T> for Vec2<T> {
    type Output = Vec2<T>;

    fn sub(self, rhs: T) -> Self {
        Self { x: self.x - rhs, y: self.y - rhs }
    }
}

impl<T: std::ops::Mul<Output = T> + Copy> std::ops::Mul for Vec2<T> {
    type Output = Vec2<T>;

    fn mul(self, rhs: Self) -> Self {
        Self { x: self.x * rhs.x, y: self.y * rhs.y }
    }
}

impl<T: std::ops::Div<Output = T> + Copy> std::ops::Div<Vec2<T>> for Vec2<T> {
    type Output = Self;

    fn div(self, rhs: Self) -> Self::Output {
        Self::Output { x: self.x / rhs.x, y: self.y / rhs.y }
    }
}

impl<T: std::ops::Div<Output = T> + Copy> std::ops::Div<T> for Vec2<T> {
    type Output = Vec2<T>;

    fn div(self, rhs: T) -> Self {
        Self { x: self.x / rhs, y: self.y / rhs }
    }
}

impl<T: Copy> From<T> for Vec2<T> {
    fn from(v: T) -> Self {
        Self { x: v, y: v }
    }
}

#[derive(Debug)]
pub enum AtlasError {
    TooBig { w: i32, h: i32, cw: i32, ch: i32 },
    Full,
}

pub struct CellDims {
    pub offset: Vec2<i32>,
    pub size: Vec2<i32>,
}

#[derive(Debug)]
pub struct GridAtlas {
    pub tex: GLuint,
    cell_size: Vec2<i32>,
    cell_offset: Vec2<i32>,
    grid_size: Vec2<i32>,
    free_line: i32,
    free_column: i32,
}

impl GridAtlas {
    pub fn new(cell_size: Vec2<i32>, cell_offset: Vec2<i32>) -> Self {
        // FIXME limit atlas size by 256x256 cells

        // FIXME add guard padding back
        // ??? let atlas_cell_size = (cell_size * (GRID_ATLAS_PAD_PCT + 100) + 99) / 100;
        // ??? let cell_offset = cell_offset + (atlas_cell_size - cell_size) / 2;
        let atlas_cell_size = cell_size + cell_offset;

        let ret = Self {
            tex: unsafe { create_texture(GRID_ATLAS_SIZE, GRID_ATLAS_SIZE, PixelFormat::RGBA8) },
            cell_size: atlas_cell_size,
            cell_offset,
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

    pub fn load_glyph(&mut self, rasterized: &RasterizedGlyph) -> Result<Glyph, AtlasError> {
        if self.free_line >= self.cell_size.y {
            return Err(AtlasError::Full);
        }

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
        let off_y = self.cell_size.y - rasterized.top;

        let tex_x = off_x + column * self.cell_size.x;
        let tex_y = off_y + line * self.cell_size.y;

        if off_x < 0
            || off_y < 0
            || off_x + rasterized.width > self.cell_size.x
            || off_y + rasterized.height > self.cell_size.y
        {
            error!(
                "FIXME: glyph '{}' {},{} {}x{} doesn't fit into atlas cell size={:?} offset={:?}",
                rasterized.c,
                rasterized.left,
                rasterized.top,
                rasterized.width,
                rasterized.height,
                self.cell_size,
                self.cell_offset,
            );

            // return Err(AtlasError::TooBig {
            //     w: rasterized.width,
            //     h: rasterized.height,
            //     cw: self.cell_width,
            //     ch: self.cell_height,
            // });
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
