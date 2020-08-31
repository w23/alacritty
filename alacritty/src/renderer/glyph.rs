use super::atlas::Vec2;
use crate::config::font::{Font, FontDescription};
use crate::config::ui_config::Delta;
use crate::config::window::{StartupMode, WindowConfig};
use crate::config::Config;
use crate::gl::types::*;
use alacritty_terminal::term::CursorKey;
use crossfont::{
    FontDesc, FontKey, GlyphKey, Rasterize, RasterizedGlyph, Rasterizer, Size, Slant, Style, Weight,
};
use fnv::FnvHasher;
use log::*;
use std::collections::HashMap;
use std::hash::BuildHasherDefault;

/// `LoadGlyph` allows for copying a rasterized glyph into graphics memory.
pub trait LoadGlyph {
    /// Load the rasterized glyph into GPU memory.
    fn load_glyph(&mut self, rasterized: &RasterizedGlyph) -> Glyph;

    /// Clear any state accumulated from previous loaded glyphs.
    ///
    /// This can, for instance, be used to reset the texture Atlas.
    fn clear(&mut self, cell_size: Vec2<i32>, cell_offset: Vec2<i32>);
}

#[derive(Copy, Debug, Clone)]
pub struct Glyph {
    pub tex_id: GLuint,
    pub colored: bool,
    pub top: f32,
    pub left: f32,
    pub width: f32,
    pub height: f32,
    pub uv_bot: f32,
    pub uv_left: f32,
    pub uv_width: f32,
    pub uv_height: f32,
}

/// Naïve glyph cache.
///
/// Currently only keyed by `char`, and thus not possible to hold different
/// representations of the same code point.
pub struct GlyphCache {
    /// Cache of buffered glyphs.
    pub cache: HashMap<GlyphKey, Glyph, BuildHasherDefault<FnvHasher>>,

    /// Cache of buffered cursor glyphs.
    pub cursor_cache: HashMap<CursorKey, Glyph, BuildHasherDefault<FnvHasher>>,

    /// Rasterizer for loading new glyphs.
    rasterizer: Rasterizer,

    /// Regular font.
    pub font_key: FontKey,

    /// Bold font.
    pub bold_key: FontKey,

    /// Italic font.
    pub italic_key: FontKey,

    /// Bold italic font.
    pub bold_italic_key: FontKey,

    /// Font size.
    pub font_size: crossfont::Size,

    /// Glyph offset.
    glyph_offset: Delta<i8>,

    /// Font metrics.
    pub metrics: crossfont::Metrics,

    /// Cell size
    pub cell_size: Vec2<i32>,
}

impl GlyphCache {
    pub fn new<L>(
        mut rasterizer: Rasterizer,
        config: &Config,
        font: &Font,
        loader: &mut L,
    ) -> Result<GlyphCache, crossfont::Error>
    where
        L: LoadGlyph,
    {
        let (regular, bold, italic, bold_italic) = Self::compute_font_keys(font, &mut rasterizer)?;

        // Need to load at least one glyph for the face before calling metrics.
        // The glyph requested here ('m' at the time of writing) has no special
        // meaning.
        rasterizer.get_glyph(GlyphKey { font_key: regular, c: 'm', size: font.size })?;

        let metrics = rasterizer.metrics(regular, font.size)?;

        let (cell_width, cell_height) = Self::compute_cell_size(config, &metrics);
        let cell_size = Vec2::new(cell_width as i32, cell_height as i32);

        let mut cache = Self {
            cache: HashMap::default(),
            cursor_cache: HashMap::default(),
            rasterizer,
            font_size: font.size,
            font_key: regular,
            bold_key: bold,
            italic_key: italic,
            bold_italic_key: bold_italic,
            glyph_offset: font.glyph_offset,
            metrics,
            cell_size,
        };

        cache.clear_cache_with_common_glyphs(loader);

        Ok(cache)
    }

    /// Computes font keys for (Regular, Bold, Italic, Bold Italic).
    fn compute_font_keys(
        font: &Font,
        rasterizer: &mut Rasterizer,
    ) -> Result<(FontKey, FontKey, FontKey, FontKey), crossfont::Error> {
        let size = font.size;

        // Load regular font.
        let regular_desc = Self::make_desc(&font.normal(), Slant::Normal, Weight::Normal);

        let regular = Self::load_regular_font(rasterizer, &regular_desc, size)?;

        // Helper to load a description if it is not the `regular_desc`.
        let mut load_or_regular = |desc: FontDesc| {
            if desc == regular_desc {
                regular
            } else {
                rasterizer.load_font(&desc, size).unwrap_or_else(|_| regular)
            }
        };

        // Load bold font.
        let bold_desc = Self::make_desc(&font.bold(), Slant::Normal, Weight::Bold);

        let bold = load_or_regular(bold_desc);

        // Load italic font.
        let italic_desc = Self::make_desc(&font.italic(), Slant::Italic, Weight::Normal);

        let italic = load_or_regular(italic_desc);

        // Load bold italic font.
        let bold_italic_desc = Self::make_desc(&font.bold_italic(), Slant::Italic, Weight::Bold);

        let bold_italic = load_or_regular(bold_italic_desc);

        Ok((regular, bold, italic, bold_italic))
    }

    fn load_regular_font(
        rasterizer: &mut Rasterizer,
        description: &FontDesc,
        size: Size,
    ) -> Result<FontKey, crossfont::Error> {
        match rasterizer.load_font(description, size) {
            Ok(font) => Ok(font),
            Err(err) => {
                error!("{}", err);

                let fallback_desc =
                    Self::make_desc(&Font::default().normal(), Slant::Normal, Weight::Normal);
                rasterizer.load_font(&fallback_desc, size)
            }
        }
    }

    fn make_desc(desc: &FontDescription, slant: Slant, weight: Weight) -> FontDesc {
        let style = if let Some(ref spec) = desc.style {
            Style::Specific(spec.to_owned())
        } else {
            Style::Description { slant, weight }
        };
        FontDesc::new(desc.family.clone(), style)
    }

    fn rasterize_glyph(
        glyph_key: GlyphKey,
        rasterizer: &mut Rasterizer,
        glyph_offset: Delta<i8>,
        metrics: &crossfont::Metrics,
    ) -> RasterizedGlyph {
        let mut rasterized = rasterizer.get_glyph(glyph_key).unwrap_or_else(|_| Default::default());

        rasterized.left += i32::from(glyph_offset.x);
        rasterized.top += i32::from(glyph_offset.y);
        rasterized.top -= metrics.descent as i32;

        rasterized
    }

    pub fn get<L>(&mut self, glyph_key: GlyphKey, loader: &mut L) -> &Glyph
    where
        L: LoadGlyph,
    {
        let glyph_offset = self.glyph_offset;
        let rasterizer = &mut self.rasterizer;
        let metrics = &self.metrics;

        self.cache.entry(glyph_key).or_insert_with(|| {
            let rasterized = Self::rasterize_glyph(glyph_key, rasterizer, glyph_offset, metrics);
            loader.load_glyph(&rasterized)
        })
    }

    /// Clear currently cached data in both GL and the registry.
    pub fn clear_glyph_cache<L: LoadGlyph>(&mut self, config: &Config, loader: &mut L) {
        let (cell_width, cell_height) = Self::compute_cell_size(config, &self.metrics);
        self.cell_size = Vec2::new(cell_width as i32, cell_height as i32);
        self.cache = HashMap::default();
        self.cursor_cache = HashMap::default();
        self.clear_cache_with_common_glyphs(loader);
    }

    pub fn update_font_size<L: LoadGlyph>(
        &mut self,
        config: &Config,
        font: &Font,
        dpr: f64,
        loader: &mut L,
    ) -> Result<(), crossfont::Error> {
        // Update dpi scaling.
        self.rasterizer.update_dpr(dpr as f32);

        // Recompute font keys.
        let (regular, bold, italic, bold_italic) =
            Self::compute_font_keys(font, &mut self.rasterizer)?;

        self.rasterizer.get_glyph(GlyphKey { font_key: regular, c: 'm', size: font.size })?;
        let metrics = self.rasterizer.metrics(regular, font.size)?;

        info!("Font size changed to {:?} with DPR of {}", font.size, dpr);

        self.font_size = font.size;
        self.font_key = regular;
        self.bold_key = bold;
        self.italic_key = italic;
        self.bold_italic_key = bold_italic;
        self.metrics = metrics;

        self.clear_glyph_cache(config, loader);

        Ok(())
    }

    pub fn font_metrics(&self) -> crossfont::Metrics {
        self.metrics
    }

    /// Prefetch glyphs that are almost guaranteed to be loaded anyways.
    fn clear_cache_with_common_glyphs<L: LoadGlyph>(&mut self, loader: &mut L) {
        let glyph_offset = self.glyph_offset;
        let metrics = &self.metrics;
        let font_size = self.font_size;
        let rasterizer = &mut self.rasterizer;

        let cell_size = self.cell_size;
        let mut atlas_cell_size = self.cell_size;
        let mut atlas_cell_offset = Vec2 { x: 0, y: 0 };
        type Glyphs = Vec<(GlyphKey, RasterizedGlyph)>;
        let glyphs: Glyphs = [self.font_key, self.bold_key, self.italic_key, self.bold_italic_key]
            .iter()
            .flat_map(|font| {
                (32u8..=126u8)
                    .map(|c| {
                        let glyph_key = GlyphKey { font_key: *font, c: c as char, size: font_size };
                        let glyph =
                            Self::rasterize_glyph(glyph_key, rasterizer, glyph_offset, metrics);

                        atlas_cell_size.x = std::cmp::max( atlas_cell_size.x, glyph.left + glyph.width);
                        atlas_cell_size.y = std::cmp::max(atlas_cell_size.y, glyph.top);

												atlas_cell_offset.x = std::cmp::max(atlas_cell_offset.x, -glyph.left);
                        atlas_cell_offset.y = std::cmp::max(atlas_cell_offset.y, glyph.height - glyph.top);

                        debug!(
                            "precomp: '{}' left={} top={} w={} h={} off={:?} atlas_cell={:?} offset={:?}",
                            glyph.c,
                            glyph.left,
                            glyph.top,
                            glyph.width,
                            glyph.height,
                            glyph_offset,
                            atlas_cell_size,
                            atlas_cell_offset,
                        );

                        (glyph_key, glyph)
                    })
                    .collect::<Glyphs>()
            })
            .collect();

        info!("Max glyph size: {:?}", cell_size);

        loader.clear(atlas_cell_size, atlas_cell_offset);

        for glyph in glyphs {
            self.cache.entry(glyph.0).or_insert_with(|| loader.load_glyph(&glyph.1));
        }
    }

    /// Calculate font metrics without access to a glyph cache.
    pub fn static_metrics(font: Font, dpr: f64) -> Result<crossfont::Metrics, crossfont::Error> {
        let mut rasterizer = crossfont::Rasterizer::new(dpr as f32, font.use_thin_strokes())?;
        let regular_desc = GlyphCache::make_desc(&font.normal(), Slant::Normal, Weight::Normal);
        let regular = Self::load_regular_font(&mut rasterizer, &regular_desc, font.size)?;
        rasterizer.get_glyph(GlyphKey { font_key: regular, c: 'm', size: font.size })?;
        rasterizer.metrics(regular, font.size)
    }

    pub fn calculate_dimensions(
        window_config: &WindowConfig,
        dpr: f64,
        cell_width: f32,
        cell_height: f32,
    ) -> Option<(u32, u32)> {
        let dimensions = window_config.dimensions;

        if dimensions.columns_u32() == 0
            || dimensions.lines_u32() == 0
            || window_config.startup_mode != StartupMode::Windowed
        {
            return None;
        }

        let padding_x = f64::from(window_config.padding.x) * dpr;
        let padding_y = f64::from(window_config.padding.y) * dpr;

        // Calculate new size based on cols/lines specified in config.
        let grid_width = cell_width as u32 * dimensions.columns_u32();
        let grid_height = cell_height as u32 * dimensions.lines_u32();

        let width = padding_x.mul_add(2., f64::from(grid_width)).floor();
        let height = padding_y.mul_add(2., f64::from(grid_height)).floor();

        Some((width as u32, height as u32))
    }

    /// Calculate the cell dimensions based on font metrics.
    #[inline]
    pub fn compute_cell_size(config: &Config, metrics: &crossfont::Metrics) -> (f32, f32) {
        let offset_x = f64::from(config.ui_config.font.offset.x);
        let offset_y = f64::from(config.ui_config.font.offset.y);
        (
            ((metrics.average_advance + offset_x) as f32).floor().max(1.),
            ((metrics.line_height + offset_y) as f32).floor().max(1.),
        )
    }
}
