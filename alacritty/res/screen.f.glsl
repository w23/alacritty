#version 300 es
precision mediump float;

layout(location = 0) out vec4 color;
uniform sampler2D u_glyph_ref;
uniform sampler2D u_atlas;
uniform sampler2D u_color_fg;
uniform sampler2D u_color_bg;
uniform vec4 u_screen_dim; // .xy = padding, .zw = resolution
uniform vec2 u_cell_dim;
uniform vec4 u_atlas_dim; // .xy = offset, .zw = cell_size
uniform vec4 u_cursor;
uniform vec3 u_cursor_color;
uniform bool u_main_pass;

const float MAX_BIT = 127.;
const float COLOR_BIT = 16.;
const float BOTTOM_BIT = 8.;
const float TOP_BIT = 4.;
const float RIGHT_BIT = 2.;
const float LEFT_BIT = 1.;

// GLSL ES 1.00 is awesome
struct GlyphBits {
	bool glyph, color, left, right, top, bottom;
};

GlyphBits parseBits(float bits) {
	GlyphBits ret;

	ret.glyph = bits >= MAX_BIT;
	if (ret.glyph) bits -= MAX_BIT;

	ret.color = bits >= COLOR_BIT;
	if (ret.color) bits -= COLOR_BIT;

	ret.bottom = bits >= BOTTOM_BIT;
	if (ret.bottom) bits -= BOTTOM_BIT;

	ret.top = bits >= TOP_BIT;
	if (ret.top) bits -= TOP_BIT;

	ret.right = bits >= RIGHT_BIT;
	if (ret.right) bits -= RIGHT_BIT;

	ret.left = bits >= LEFT_BIT;
	if (ret.left) bits -= LEFT_BIT;

	return ret;
}

vec4 blendGlyphPixel(vec4 glyph_ref, GlyphBits bits, vec2 cell_pix, vec3 fg, vec4 dst) {
	vec2 atlas_pix = glyph_ref.xy * u_atlas_dim.zw + u_atlas_dim.xy + cell_pix;
	vec4 glyph = texture(u_atlas, atlas_pix / vec2(textureSize(u_atlas, 0)));
	vec3 mask;

	if (bits.color) {
		// Colored glyph (e.g. emoji)
		mask = vec3(glyph.a);
		glyph.rgb /= glyph.a;
	} else {
		// Regular non-colored glyph
		mask = glyph.rgb;
		// TODO is there a better way to alpha than just r
		glyph.a = glyph.r;
	}

	return vec4(mix(dst.rgb, fg, mask.rgb), dst.a + glyph.a);
}

/* bool isEmpty(vec4 glyph_ref) { */
/* 	//return glyph_ref.xy == vec2(0.); */
/* 	return glyph_ref.z == 0.; */
/* } */

void main() {
	vec2 uv = gl_FragCoord.xy;
	uv.y = u_screen_dim.w - uv.y;
	uv.xy -= u_screen_dim.xy;

	vec2 cell = floor(uv / u_cell_dim);
	vec2 screen_cells = vec2(textureSize(u_glyph_ref, 0));

	if (any(lessThan(uv.xy, vec2(0.)))
			|| any(greaterThanEqual(cell, screen_cells))
	) {
		color = vec4(1., 0., 0., 1.);
		return;
	}

	vec2 tuv = (cell + .5) / screen_cells;
	vec2 cell_pix = mod(uv, u_cell_dim);

	if (u_main_pass) {
		vec4 bg = texture(u_color_bg, tuv);
		color = bg;
		vec3 mask;
		if (cell == u_cursor.xy) {
			color = blendGlyphPixel(vec4(u_cursor.zw, 0., 0.), parseBits(MAX_BIT), cell_pix, u_cursor_color, color);
		}
	} else {
		color = vec4(0.);
		//if (empty) discard;
		//if (!empty) color = vec4(.5);
	}

	// FIXME: discard on non-main, return on main IF there are no overlapping glyph parts from neighbour grid cells
	/* if (glyph.xy == vec2(0.)) { */
	/* 		return; */
	/* } */

	// This cell glyph
	vec4 glyph = texture(u_glyph_ref, tuv) * 255.;;
	GlyphBits bits = parseBits(glyph.z);
	if (bits.glyph) {
		vec3 fg = texture(u_color_fg, tuv).rgb;
		color = blendGlyphPixel(glyph, bits, cell_pix, fg, color);
	}

	// Neighbour cells overlappery
	// TODO: glyph-level (outside shader) masking for these cases so that we only check some bits
	// instead of brute-forcing all cases

	// TODO:
	// -, +
	// +. +
	// -, 0
	// +, 0
	// -, -
	// 0, -
	// +, -

	if (cell_pix.y > (u_cell_dim.y - u_atlas_dim.y) && cell.y < (screen_cells.y-1.)) {
		vec2 tuv = (cell + vec2(.5, 1.5)) / screen_cells;
		vec4 glyph = texture(u_glyph_ref, tuv) * 255.;
		vec3 fg = texture(u_color_fg, tuv).rgb;
		color = blendGlyphPixel(glyph, parseBits(glyph.z), cell_pix + vec2(0., -u_cell_dim.y), fg, color);
		//color.g = 1.;
	}

	if (bits.left && cell.x > 0. && cell_pix.x < (u_atlas_dim.z - u_atlas_dim.x - u_cell_dim.x)) {
		vec2 tuv = (cell + vec2(-.5, .5)) / screen_cells;
		vec4 glyph = texture(u_glyph_ref, tuv) * 255.;
		vec3 fg = texture(u_color_fg, tuv).rgb;
		color = blendGlyphPixel(glyph, parseBits(glyph.z), cell_pix + vec2(u_cell_dim.x, 0.), fg, color);
		color.b = 1.;
	}

	//color = vec4(fg.rgb, 1.);
	//color = vec4(bg.rgb, 1.);
	//color = vec4(mask.rgb, 1.);
	//color = vec4(u_cursor_color, 1.);
	//color = mix(color, vec4(texture(u_atlas, uv / vec2(textureSize(u_atlas, 0))).rgb, 1.), 1.);

//#define ATLAS
#ifdef ATLAS
	{
		color = vec4(0., 0., 0., 1.);
		vec2 cp = mod(uv, u_atlas_dim.zw);
		color.rg = fract(uv / u_atlas_dim.zw);
		color.rgb += texture(u_atlas, uv / vec2(textureSize(u_atlas, 0))).rgb;
		color.b += step(cp.y, u_atlas_dim.y);
		color.b += step(cp.x, u_atlas_dim.x);
		//color.b += step(cp.y, 15.);
		//color = mix(color, vec4(, 1.), 1.);
	}
#endif
}
