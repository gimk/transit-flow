shader_type canvas_item;
// A person seen from above (PedestrianRenderer): shoulders across in the instance colour
// (their clothes), the head on top, and a dark outline so they read against sidewalks
// and crosswalks. UV holds body coordinates from -0.5 to 0.5, x the way they face; the
// instance is scaled to PedestrianRenderer.FIGURE_SIZE (px), which this layout fills.

const vec2 SHOULDERS = vec2(1.0, 1.8); // Half extents, px.
const float HEAD = 0.85;
const vec2 HEAD_AT = vec2(0.15, 0.0);
const float OUTLINE = 0.6;
const vec3 HAIR = vec3(0.2, 0.14, 0.1);
const vec3 OUTLINE_COLOR = vec3(0.08, 0.08, 0.1);

void fragment() {
	vec2 size = 2.0 * (SHOULDERS + OUTLINE);
	vec2 q = UV * size;
	float aa = max(fwidth(q.x), 0.001) * 0.6;
	float body = (length(q / SHOULDERS) - 1.0) * min(SHOULDERS.x, SHOULDERS.y);
	float head = length(q - HEAD_AT) - HEAD;
	float figure = min(body, head);
	vec3 color = mix(HAIR, COLOR.rgb, smoothstep(-aa, aa, head));
	color = mix(color, OUTLINE_COLOR, smoothstep(-aa, aa, figure));
	float alpha = 1.0 - smoothstep(OUTLINE - aa, OUTLINE + aa, figure);
	COLOR = vec4(color, COLOR.a * alpha);
}
