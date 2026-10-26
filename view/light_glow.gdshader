shader_type canvas_item;
render_mode blend_add;
// A soft pool of light (NightLights): bright at the centre, fading out to the quad's
// edge, in the instance colour, scaled by how dark it is. Added on top of the world, so
// it brightens what's under it; divided by the world's tint (DayNight.applied_tint) so
// the night doesn't darken it. UV runs -0.5..0.5 across the quad.

uniform float night = 0.0;
uniform vec3 tint = vec3(1.0);

void fragment() {
	float d = length(UV) * 2.0;
	float glow = pow(max(1.0 - d, 0.0), 2.2);
	// A small bright core: the lamp or window itself.
	float core = 1.0 - smoothstep(0.03, 0.06, d);
	COLOR = vec4(COLOR.rgb / max(tint, vec3(0.05)), (glow * 0.75 + core * 0.7) * COLOR.a * night);
}
