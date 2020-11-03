#version 330 core

layout(location = 0, index = 0) out vec4 color;
uniform sampler2D glyphRef;
uniform sampler2D atlas;
uniform sampler2D color_fg;
uniform sampler2D color_bg;
uniform vec4 screenDim;
uniform vec2 cellDim;
uniform vec4 cursor;
uniform vec3 cursor_color;
uniform float t;

vec3 drawGlyph(vec4 glyph, vec2 cell_uv, vec3 bg, vec3 fg) {
	vec2 atlas_pix = (glyph.xy + cell_uv) * cellDim;
	vec4 mask = texture(atlas, atlas_pix / textureSize(atlas, 0));

	//return mask.rgb;

	if (glyph.z > 0.)
		return mix(bg, mask.rgb, mask.a);
	else
		return mix(bg, fg.rgb, mask.rgb);
}

float h1(float v) { return fract(sin(v)*43857.2345); }
float h2(vec2 v) { return h1(dot(v, vec2(17.342, 45.3546))); }

float n1(float v) {
	float V = floor(v); v = fract(v);
	return mix(h1(V), h1(V+1.), v);
}

float n2(vec2 v) {
	vec2 V = floor(v); v = fract(v);
	vec2 e=vec2(0.,1.);
	return mix(
			mix(h2(V), h2(V+e.xy), v.y),
			mix(h2(V+e.yx), h2(V+e.yy), v.y), v.x);
}

float fn2(vec2 v) {
	return .5*n2(v)+.25*n2(v*1.7) + .12 * n2(v*9.);
}

mat2 Rm(float a) { float c=cos(a),s=sin(a); return mat2(c,s,-s,c); }

vec3 picsel(vec2 uv) {
	uv.y = screenDim.w - uv.y;
	uv.xy -= screenDim.xy;

	vec2 cell = floor(uv / cellDim);
	vec2 screen_cells = textureSize(glyphRef, 0);

		float a = .4 + .2 * n1(t), b = a + .3 * n1(t+.5);

		float ag = length(uv);
		vec2 tuv = uv * Rm(ag * .0005 * sin(t*.13)*2. + t * .3);// vec2(cos(ag), sin(ag))

		//vec3 c = vec3(0., 0., 0.);
		vec2 p = tuv/100.;
		vec3 c = vec3(.9,.8,.7) *
				(step(fract(p.x-.5), .02) + step(fract(p.y-.5), .02)) *
				step(length(fract(p)-.5), .3) * smoothstep(a, b, n2(floor(p)));

		p = tuv / 50.;
		p.x -= t;

		a = .5 + .2 * n1(t*2.);
		b = a + .3 * n1(t*1.7+.5);

		c += vec3(.6,.7,.8) *
				(step(fract(p.x-.5), .03) + step(fract(p.y-.5), .03)) *
				step(length(fract(p)-.5), .2) * smoothstep(a, b, n2(floor(p)));

		p = tuv / 20.;
		p.x -= t * 4.;

		a = .5 + .2 * n1(t*1.3+4.);
		b = a + .3 * n1(t*1.7+.5);

		c += .4 * vec3(.8,.4,.7) *
				(step(fract(p.x-.5), .07) + step(fract(p.y-.5), .07)) *
				step(length(fract(p)-.5), .4) * smoothstep(a, b, n2(floor(p)));

		if (any(lessThan(uv.xy, vec2(0.)))
				|| any(greaterThanEqual(cell, screen_cells))
		) {
			return c;
		}

	vec2 cell_uv = fract(uv / cellDim);

	/* float h = screen_cells.y * h1(cell.x+floor(t)), */
	/* 			y = screen_cells.y * h1(cell.x + 72.-floor(t)); */
	/* cell.y += */
	/* 	- step(.9, n1(cell.x+t)) * mod(t*4., 5.) */
	/* 	* step(y, cell.y) * step(cell.y, y+h); */

	tuv = (cell + .5) / vec2(textureSize(glyphRef, 0));
	vec4 glyph = texture(glyphRef, tuv);
	vec3 fg = texture(color_fg, tuv).rgb;
	vec3 bg = texture(color_bg, tuv).rgb;

	vec3 color = bg;

	if (cell == cursor.xy)
		color = drawGlyph(vec4(cursor.zw, 0., 0.), cell_uv, color, cursor_color);

	glyph.xy *= 255.;

	//glyph.x += step(.9, n2(cell + floor(t))) * (16. * n2(tuv*100.));
	//glyph.x += step(.99, h2(cell+mod(floor(t)*17.,5.)));// * (16. * n2(tuv*100.);

	return mix(c, /* (.8 + .4*h2(cell+floor(fract(t)*128.))) * */ drawGlyph(vec4(glyph.xy, glyph.zw), cell_uv, color, fg.rgb), .8);
	//return drawGlyph(vec4(glyph.xy, glyph.zw), cell_uv, color, fg.rgb);
}

void main() {
	vec2 uv = gl_FragCoord.xy;

	vec2 cuv = gl_FragCoord.xy / screenDim.zw * 2. - 1.;
	cuv.x *= screenDim.z / screenDim.w;
	float r = length(cuv);

	//uv.xy += 32. * sin(uv.xy * .05 / screenDim.xy).yx;

	uv.xy = floor(uv.xy / 2.) * 2.;

	// color = vec4(
	// 		//cuv, 0.,
	// 		picsel(uv + normalize(cuv) * r * 17.).r,
	// 		picsel(uv + normalize(cuv) * r * 19.).g,
	// 		picsel(uv + normalize(cuv) * r * 23.).b,
	// 		1.);

	float s = uv.x * uv.y;
	vec3 c = vec3(0.);

	float N = 8.;
	for (float j = 0.; j < N; ++j) {
		vec3 ac = vec3(1.);

		float la = h1(s+=j) * 6.28;
		float lr = .05 * sqrt(h1(s+=.5));
		float f = 2.29; //3. + 2. * (n1(t) - .5);
		float fov = .5;

		vec3 at = vec3(cuv*fov, -1.) * f, O, D;

			O = lr * vec3(cos(la), sin(la), 0.);
			O.xz *= Rm(.2 + .1 * sin(t*.7));
			O.yz *= Rm(-.2 - .1 *cos(t*.3));
			D = normalize(at - O);
			/* D.xz *= Rm(.2 + .1 * sin(t*.7)); */
			/* D.yz *= Rm(-.2 - .1 *cos(t*.3)); */
			O += vec3(-1.2, .9, 2.4);

		for (float i = 0.; i < 4.; ++i) {
			s = mod(i+s, 100.);
			float l = D.z < 0. ? - O.z / D.z : 1e6;
			vec3 N = vec3(0., 0., 1.), p = O;

			vec3 me = vec3(0.), ma = vec3(1.);
			float r = .2;

			float yl = - O.y / D.y;
			if (yl > 0. && yl < l) {
				N = vec3(0., 1., 0.);
				l = yl;
				p = O + D * l;
				r = .01 + .08 * smoothstep(.45, .55, fn2(p.xz * 4.));
			} else {
				if (l > 100.)
					break;
				p = O + D * l;
				float rl = length(p.xy);
				vec2 np = normalize(p.xy);
				vec2 puv = (p.xy + vec2(2.5, 0.)) * 512.;
				me = 1.5 * vec3(
						picsel(puv + rl * np * .8).r,
						picsel(puv + rl * np * 1.6).g,
						picsel(puv).b
					);
				r = .8;
			}

			c += me * ac;
			ac *= ma;

			O = p + N * .01;
			D = mix(
					reflect(D, N),
					normalize(vec3(h1(s+=p.y),h1(s+=p.z),h1(s-=p.x))-.5), r);

			D *= sign(dot(D, N));
		}
	}

	color = vec4(c/N, 1.);

	//color *= (.1 + .9 * fract(gl_FragCoord.y/4.));

	//color = vec4(fg.rgb, 1.);
	//color = vec4(bg.rgb, 1.);
	//color = vec4(mask.rgb, 1.);
	//color = vec4(cursor_color, 1.);
	//color = mix(color, vec4(texture(atlas, uv / textureSize(atlas, 0)).rgb, 1), 1.);
}
