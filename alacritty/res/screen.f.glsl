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

vec4 drawGlyph(vec4 glyph, vec2 cell_pix, vec3 bg, vec3 fg) {
	vec2 atlas_pix = glyph.xy * u_atlas_dim.zw + u_atlas_dim.xy + cell_pix;
	vec4 mask = texture(u_atlas, atlas_pix / vec2(textureSize(u_atlas, 0)));

	//return mask.rgb;

	if (glyph.z > 0.) {
		return vec4(mix(bg, mask.rgb, mask.a), mask.a);
	} else
		return vec4(mix(bg, fg.rgb, mask.rgb), mask.r);
}

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
	vec4 glyph = texture(u_glyph_ref, tuv);
	vec3 fg = texture(u_color_fg, tuv).rgb;
	vec3 bg = texture(u_color_bg, tuv).rgb;

	if (glyph.xy == vec2(0.)) {
		//discard;
		//return;
	}

	color = vec4(bg, 1.);

	vec2 cell_pix = mod(uv, u_cell_dim);
	if (cell == u_cursor.xy)
		color = drawGlyph(vec4(u_cursor.zw, 0., 0.), cell_pix, color.rgb, u_cursor_color);
	color = drawGlyph(vec4(glyph.xy * 255., glyph.zw), cell_pix, color.rgb, fg.rgb);

	if (cell_pix.y > (u_cell_dim.y - u_atlas_dim.y) && cell.y < (screen_cells.y-1.)) {
		vec2 tuv = (cell + vec2(.5, 1.5)) / screen_cells;
		vec4 glyph = texture(u_glyph_ref, tuv);
		vec3 fg = texture(u_color_fg, tuv).rgb;
		vec3 bg = texture(u_color_bg, tuv).rgb;
		vec4 c = drawGlyph(vec4(glyph.xy * 255., glyph.zw), cell_pix + vec2(0., -u_cell_dim.y), color.rgb, fg.rgb);
		color = mix(color, c, c.a);
		//color.g = 1.;
	}

	if (cell_pix.x < (u_atlas_dim.z - u_cell_dim.x) && cell.x > 0.) {
		vec2 tuv = (cell + vec2(-.5, .5)) / screen_cells;
		vec4 glyph = texture(u_glyph_ref, tuv);
		vec3 fg = texture(u_color_fg, tuv).rgb;
		vec3 bg = texture(u_color_bg, tuv).rgb;
		vec4 c = drawGlyph(vec4(glyph.xy * 255., glyph.zw), cell_pix + vec2(u_cell_dim.x, 0.), color.rgb, fg.rgb);
		//color.b = 1.;
		color = mix(color, c, c.a);
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
	//color = vec4(u_cursor_color, 1.);
	//color = mix(color, vec4(texture(u_atlas, uv / vec2(textureSize(u_atlas, 0))).rgb, 1.), 1.);

//#define ATLAS
#ifdef ATLAS
	{
		color = vec4(0., 0., 0., 1.);
		vec2 cp = mod(uv, u_atlas_dim.zw);
		//color.rg = fract(uv / u_atlas_dim.zw);
		color.rgb += texture(u_atlas, uv / vec2(textureSize(u_atlas, 0))).rgb;
		//color.b += step(cp.y, u_atlas_dim.y);
		//color.b += step(cp.y, 15.);
		//color = mix(color, vec4(, 1.), 1.);
	}
#endif
}
