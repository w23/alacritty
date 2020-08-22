#version 300 es
precision mediump float;

layout(location = 0) out vec4 color;
uniform sampler2D glyph_ref;
uniform sampler2D atlas;
uniform sampler2D color_fg;
uniform sampler2D color_bg;
uniform vec4 screen_dim; // .xy = padding, .zw = resolution
uniform vec2 cell_dim;
uniform vec4 atlas_dim; // .xy = offset, .zw = cell_size
uniform vec4 cursor;
uniform vec3 cursor_color;

vec3 drawGlyph(vec4 glyph, vec2 cell_pix, vec3 bg, vec3 fg) {
	vec2 atlas_pix = glyph.xy * atlas_dim.zw + atlas_dim.xy + cell_pix;
	vec4 mask = texture(atlas, atlas_pix / vec2(textureSize(atlas, 0)));

	//return mask.rgb;

	if (glyph.z > 0.)
		return mix(bg, mask.rgb, mask.a);
	else
		return mix(bg, fg.rgb, mask.rgb);
}

void main() {
	vec2 uv = gl_FragCoord.xy;
	uv.y = screen_dim.w - uv.y;
	uv.xy -= screen_dim.xy;

	vec2 cell = floor(uv / cell_dim);
	vec2 screen_cells = vec2(textureSize(glyph_ref, 0));

	if (any(lessThan(uv.xy, vec2(0.)))
			|| any(greaterThanEqual(cell, screen_cells))
	) {
		color = vec4(1., 0., 0., 1.);
		return;
	}

	vec2 tuv = (cell + .5) / screen_cells;
	vec4 glyph = texture(glyph_ref, tuv);
	vec3 fg = texture(color_fg, tuv).rgb;
	vec3 bg = texture(color_bg, tuv).rgb;

	color = vec4(bg, 1.);

	vec2 cell_pix = mod(uv, cell_dim);
	if (cell == cursor.xy)
		color = vec4(drawGlyph(vec4(cursor.zw, 0., 0.), cell_pix, color.rgb, cursor_color), 1.);

	color = vec4(drawGlyph(vec4(glyph.xy * 255., glyph.zw), cell_pix, color.rgb, fg.rgb), 1.);

	if (cell_pix.y > (cell_dim.y - atlas_dim.y) && cell.y < (screen_cells.y-1.)) {
		vec2 tuv = (cell + vec2(.5, 1.5)) / screen_cells;
		vec4 glyph = texture(glyph_ref, tuv);
		vec3 fg = texture(color_fg, tuv).rgb;
		vec3 bg = texture(color_bg, tuv).rgb;
		color = vec4(drawGlyph(vec4(glyph.xy * 255., glyph.zw), cell_pix + vec2(0., -cell_dim.y), color.rgb, fg.rgb), 1.);
		//color.g = 1.;
	}

	if (cell_pix.x < atlas_dim.x && cell.x > 0.) {
		vec2 tuv = (cell + vec2(-.5, .5)) / screen_cells;
		vec4 glyph = texture(glyph_ref, tuv);
		vec3 fg = texture(color_fg, tuv).rgb;
		vec3 bg = texture(color_bg, tuv).rgb;
		color = vec4(drawGlyph(vec4(glyph.xy * 255., glyph.zw), cell_pix + vec2(cell_dim.x, 0.), color.rgb, fg.rgb), 1.);
		//color.b = 1.;
	}

	// TODO:
	// -, +
	// +. +
	// -, 0
	// +, 0
	// -, -
	// 0, -
	// +, -

	// TODO: glyph-level (outside shader) masking for these cases so that we only check some bits
	// instead of brute-forcing all cases

	//color = vec4(fg.rgb, 1.);
	//color = vec4(bg.rgb, 1.);
	//color = vec4(mask.rgb, 1.);
	//color = vec4(cursor_color, 1.);
	//color = mix(color, vec4(texture(atlas, uv / textureSize(atlas, 0)).rgb, 1), 1.);
}
