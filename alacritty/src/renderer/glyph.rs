use crate::config::font::{Font, FontDescription};
use crate::config::ui_config::Delta;
use crate::config::window::{StartupMode, WindowConfig};
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
    fn clear(&mut self);
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
}

impl GlyphCache {
    pub fn new<L>(
        mut rasterizer: Rasterizer,
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
        };

        cache.load_common_glyphs(loader);

        Ok(cache)
    }

    fn load_glyphs_for_font<L: LoadGlyph>(&mut self, font: FontKey, loader: &mut L) {
        let size = self.font_size;
        for i in 32u8..=126u8 {
            self.get(GlyphKey { font_key: font, c: i as char, size }, loader);
        }
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

    pub fn get<L>(&mut self, glyph_key: GlyphKey, loader: &mut L) -> &Glyph
    where
        L: LoadGlyph,
    {
        let glyph_offset = self.glyph_offset;
        let rasterizer = &mut self.rasterizer;
        let metrics = &self.metrics;
        self.cache.entry(glyph_key).or_insert_with(|| {
            let mut rasterized =
                rasterizer.get_glyph(glyph_key).unwrap_or_else(|_| Default::default());

            rasterized.left += i32::from(glyph_offset.x);
            rasterized.top += i32::from(glyph_offset.y);
            rasterized.top -= metrics.descent as i32;

            loader.load_glyph(&rasterized)
        })
    }

    /// Clear currently cached data in both GL and the registry.
    pub fn clear_glyph_cache<L: LoadGlyph>(&mut self, loader: &mut L) {
        loader.clear();
        self.cache = HashMap::default();
        self.cursor_cache = HashMap::default();

        self.load_common_glyphs(loader);
    }

    pub fn update_font_size<L: LoadGlyph>(
        &mut self,
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

        self.clear_glyph_cache(loader);

        Ok(())
    }

    pub fn font_metrics(&self) -> crossfont::Metrics {
        self.metrics
    }

    /// Prefetch glyphs that are almost guaranteed to be loaded anyways.
    fn load_common_glyphs<L: LoadGlyph>(&mut self, loader: &mut L) {
        // FIXME: simple render doesn't know about cell size at this point, so we can't preload glyphs now
        // self.load_glyphs_for_font(self.font_key, loader);
        // self.load_glyphs_for_font(self.bold_italic_key, loader);
        // self.load_glyphs_for_font(self.italic_key, loader);
        // self.load_glyphs_for_font(self.bold_italic_key, loader);
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
}
