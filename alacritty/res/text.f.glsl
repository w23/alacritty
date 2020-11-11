#version 330 core
uniform sampler2D u_atlas;

smooth in vec2 uv;
flat in vec3 fg;
flat in float flags;

//out vec4 FragColor;
layout(location = 0, index = 0) out vec4 color;
layout(location = 0, index = 1) out vec4 alphaMask;

void main() {
		//FragColor = vec4(uv,0.,.4); return;
		vec4 mask = texture(u_atlas, uv);
		bool colored = flags > 0.;
		if (colored) {
			alphaMask = vec4(mask.a);
			color = vec4(0.);
			if (mask.a > 0.) {
				color = vec4(mask.rgb/mask.a, 1.);
			}
		} else {
			alphaMask = mask.rgbr;
			color = vec4(fg, 1.);
		}
		// alphaMask = vec4(1.);
		// mask = vec4(1.);
}
