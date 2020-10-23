use crate::cursor;
use {
    crate::{
        config::ui_config::UIConfig,
        gl,
        renderer::{rects::RenderRect, Error, GlyphCache, LoadGlyph},
    },
    alacritty_terminal::{
        index::{Column, Line},
        term::{
            self,
            cell::{self, Flags},
            color::Rgb,
            RenderableCell, RenderableCellContent, SizeInfo,
        },
    },
};

use alacritty_terminal::config::Cursor;

use super::glyph::{AtlasRef, Glyph, GlyphKey, RasterizedGlyph};
use super::glyphrect::{GlyphQuad, QuadGlyphRenderer};
use super::grid::GridGlyphRenderer;
use super::math::*;
use super::solidrect::SolidRectRenderer;

#[derive(Debug)]
pub struct SimpleRenderer {
    // Fast grid-based glyph renderer. Used for majority of the glyphs
    // Also used to draw backgound color
    grids: GridGlyphRenderer,

    // Slower quad-based glyph renderer. Used for:
    // - zero-width characters which are not well aligned with grid
    // - wide characters (TODO: draw them using grid-based renderer also)
    // - characters too big for cell-based renderer
    quad_glyphs: QuadGlyphRenderer,

    // Solid-color rects
    // FULL BRIDGE
    solid_rects: SolidRectRenderer,
}

impl SimpleRenderer {
    pub fn new() -> Result<SimpleRenderer, Error> {
        Ok(Self {
            grids: GridGlyphRenderer::new()?,
            quad_glyphs: QuadGlyphRenderer::new(),
            solid_rects: SolidRectRenderer::new()?,
        })
    }

    pub fn begin<'a>(
        &'a mut self,
        config: &'a UIConfig,
        cursor_config: Cursor,
        size_info: &'a SizeInfo,
    ) -> RenderContext<'a> {
        RenderContext { this: self, size_info, config, cursor_config }
    }

    pub fn with_loader<F, T>(&mut self, func: F) -> T
    where
        F: FnOnce(LoaderApi<'_>) -> T,
    {
        unsafe {
            // FIXME needed?
            gl::ActiveTexture(gl::TEXTURE0);
        }

        func(LoaderApi { renderer: self })
    }

    pub fn resize(&mut self, size: &term::SizeInfo) {
        self.grids.resize(size);
    }

    pub fn clear(&mut self, color: Rgb, background_opacity: f32) {
        self.quad_glyphs.clear();
        self.grids.clear(color, background_opacity);

        unsafe {
            gl::ClearColor(0.0, 0.0, 0.0, 0.0);
            gl::Clear(gl::COLOR_BUFFER_BIT);
        }
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    pub fn finish(&self) {
        unsafe {
            gl::Finish();
        }
    }
}

impl LoadGlyph for SimpleRenderer {
    fn load_glyph(&mut self, rasterized: &RasterizedGlyph) -> Glyph {
        match self.grids.load_glyph(rasterized) {
            Some(glyph) => glyph,
            None => self.quad_glyphs.insert_into_atlas(rasterized),
        }
    }

    fn clear(&mut self, cell_size: Vec2<i32>, cell_offset: Vec2<i32>) {
        self.grids.clear_atlas(cell_size, cell_offset);
        self.quad_glyphs.clear_atlas();
    }
}

#[derive(Debug)]
pub struct RenderContext<'a> {
    this: &'a mut SimpleRenderer,
    size_info: &'a term::SizeInfo,
    config: &'a UIConfig,
    cursor_config: Cursor,
}

impl<'a> RenderContext<'a> {
    /// Render a string in a variable location. Used for printing the render timer, warnings and
    /// errors.
    pub fn render_string(
        &mut self,
        glyph_cache: &mut GlyphCache,
        line: Line,
        string: &str,
        fg: Rgb,
        bg: Option<Rgb>,
    ) {
        let bg_alpha = bg.map(|_| 1.0).unwrap_or(0.0);

        let cells = string
            .chars()
            .enumerate()
            .map(|(i, c)| RenderableCell {
                line,
                column: Column(i),
                inner: RenderableCellContent::Chars({
                    let mut chars = [' '; cell::MAX_ZEROWIDTH_CHARS + 1];
                    chars[0] = c;
                    chars
                }),
                flags: Flags::empty(),
                bg_alpha,
                fg,
                bg: bg.unwrap_or(Rgb { r: 0, g: 0, b: 0 }),
            })
            .collect::<Vec<_>>();

        for cell in cells {
            self.update_cell(cell, glyph_cache);
        }
    }

    pub fn update_cell(&mut self, cell: RenderableCell, glyph_cache: &mut GlyphCache) {
        let wide = match cell.flags & Flags::WIDE_CHAR {
            Flags::WIDE_CHAR => true,
            _ => false,
        };

        match cell.inner {
            RenderableCellContent::Cursor(cursor_key) => {
                // Raw cell pixel buffers like cursors don't need to go through font lookup.
                let metrics = glyph_cache.metrics;
                let glyph = glyph_cache.cursor_cache.entry(cursor_key).or_insert_with(|| {
                    self.load_glyph(&RasterizedGlyph {
                        wide,
                        zero_width: false,
                        rasterized: cursor::get_cursor_glyph(
                            cursor_key.style,
                            metrics,
                            self.config.font.offset.x,
                            self.config.font.offset.y,
                            cursor_key.is_wide,
                            self.cursor_config.thickness(),
                        ),
                    })
                });

                match glyph.atlas_ref {
                    AtlasRef::Grid(grid) => {
                        self.this.grids.set_cursor(
                            glyph.atlas_index,
                            cell.column.0 as i32,
                            cell.line.0 as i32,
                            grid.column as f32,
                            grid.line as f32,
                            cell.fg,
                        );
                    }

                    AtlasRef::Free(free) => {
                        let glyph_quad = GlyphQuad {
                            atlas_index: glyph.atlas_index,
                            pos: Vec2::<i16> {
                                x: cell.column.0 as i16 * self.size_info.cell_width as i16,
                                y: cell.line.0 as i16 * self.size_info.cell_height as i16,
                            },
                            geom: free,
                            fg: cell.fg,
                            colored: glyph.colored,
                        };

                        self.this.quad_glyphs.add_to_render(self.size_info, &glyph_quad);
                    }
                }
            }

            // こんにちは
            RenderableCellContent::Chars(chars) => {
                // Get font key for cell.
                let font_key = match cell.flags & Flags::BOLD_ITALIC {
                    Flags::BOLD_ITALIC => glyph_cache.bold_italic_key,
                    Flags::ITALIC => glyph_cache.italic_key,
                    Flags::BOLD => glyph_cache.bold_key,
                    _ => glyph_cache.font_key,
                };

                // Don't render text of HIDDEN cells.
                let mut chars = if cell.flags.contains(Flags::HIDDEN) {
                    [' '; cell::MAX_ZEROWIDTH_CHARS + 1]
                } else {
                    chars
                };

                // Render tabs as spaces in case the font doesn't support it.
                if chars[0] == '\t' {
                    chars[0] = ' ';
                }

                self.this.grids.update_cell_colors(&cell, wide);

                self.push_char(
                    GlyphKey {
                        wide,
                        zero_width: false,
                        key: crossfont::GlyphKey {
                            font_key,
                            size: glyph_cache.font_size,
                            c: chars[0],
                        },
                    },
                    &cell,
                    glyph_cache,
                    false,
                );

                // Render zero-width characters.
                for c in (&chars[1..]).iter().filter(|c| **c != ' ') {
                    self.push_char(
                        GlyphKey {
                            wide,
                            zero_width: true,
                            key: crossfont::GlyphKey {
                                font_key,
                                size: glyph_cache.font_size,
                                c: *c,
                            },
                        },
                        &cell,
                        glyph_cache,
                        true,
                    );
                }
            }
        };
    }

    fn push_char(
        &mut self,
        glyph_key: GlyphKey,
        cell: &RenderableCell,
        glyph_cache: &mut GlyphCache,
        zero_width: bool,
    ) {
        let glyph = glyph_cache.get(glyph_key, self);

        match glyph.atlas_ref {
            AtlasRef::Grid(atlas_ref) => {
                self.this.grids.update_cell(cell, glyph.atlas_index, glyph.colored, atlas_ref);
            }
            AtlasRef::Free(free) => {
                let glyph_quad = GlyphQuad {
                    atlas_index: glyph.atlas_index,
                    pos: Vec2::<i16> {
                        x: (if zero_width {
                            // The metrics of zero-width characters are based on rendering
                            // the character after the current cell, with the anchor at the
                            // right side of the preceding character. Since we render the
                            // zero-width characters inside the preceding character, the
                            // anchor has been moved to the right by one cell.
                            // FIXME WHY????
                            1
                        } else {
                            0
                        } + cell.column.0 as i16)
                            * self.size_info.cell_width as i16,
                        y: cell.line.0 as i16 * self.size_info.cell_height as i16,
                    },
                    geom: free,
                    fg: cell.fg,
                    colored: glyph.colored,
                };

                self.this.quad_glyphs.add_to_render(self.size_info, &glyph_quad);
            }
        }
    }

    /// Draw all rectangles simultaneously to prevent excessive program swaps.
    pub fn draw_rects(&mut self, size_info: &term::SizeInfo, rects: Vec<RenderRect>) {
        self.this.solid_rects.draw(size_info, rects);
    }

    pub fn draw_text(&mut self) {
        self.this.grids.draw(self.size_info);
        self.this.quad_glyphs.draw(self.size_info);
    }
}

impl<'a> LoadGlyph for RenderContext<'a> {
    fn load_glyph(&mut self, rasterized: &RasterizedGlyph) -> Glyph {
        self.this.load_glyph(rasterized)
    }

    fn clear(&mut self, cell_size: Vec2<i32>, cell_offset: Vec2<i32>) {
        LoadGlyph::clear(self.this, cell_size, cell_offset);
    }
}

#[derive(Debug)]
pub struct LoaderApi<'a> {
    renderer: &'a mut SimpleRenderer,
}

impl<'a> LoadGlyph for LoaderApi<'a> {
    fn load_glyph(&mut self, rasterized: &RasterizedGlyph) -> Glyph {
        self.renderer.load_glyph(rasterized)
    }

    fn clear(&mut self, cell_size: Vec2<i32>, cell_offset: Vec2<i32>) {
        LoadGlyph::clear(self.renderer, cell_size, cell_offset);
    }
}
