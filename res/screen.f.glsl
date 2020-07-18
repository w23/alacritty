#version 330 core

//flat in vec4 bg;

layout(location = 0, index = 0) out vec4 color;
//uniform sampler2D mask;

void main()
{
	color = vec4(1., 0., 0., 0.);
    // if (backgroundPass != 0) {
    //     if (bg.a == 0.0)
    //         discard;
    //
    //     alphaMask = vec4(1.0);
    //     color = vec4(bg.rgb, 1.0);
    // } else {
    //     if (colored != 0) {
    //         // Color glyphs, like emojis.
    //         vec4 glyphColor = texture(mask, TexCoords);
    //         alphaMask = vec4(glyphColor.a);
    //
    //         // Revert alpha premultiplication.
    //         if (glyphColor.a != 0) {
    //             glyphColor.rgb = vec3(glyphColor.rgb / glyphColor.a);
    //         }
    //
    //         color = vec4(glyphColor.rgb, 1.0);
    //     } else {
    //         // Regular text glyphs.
    //         vec3 textColor = texture(mask, TexCoords).rgb;
    //         alphaMask = vec4(textColor, textColor.r);
    //         color = vec4(fg, 1.0);
    //     }
    // }
}
