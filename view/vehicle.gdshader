shader_type canvas_item;
// Draws a vehicle seen from above inside its instance quad (VehicleRenderer): body in
// the instance colour, glass, roof details per kind, lamps. The quad reaches past the
// front bumper and to the sides, for the headlight beams. UV holds body coordinates:
// x from -0.5 (rear) to 0.5 (front), y from -0.5 (left) to 0.5 (right).
// INSTANCE_CUSTOM: x kind (Vehicle.Kind), y lamp flags (VehicleRenderer.LAMP_*),
// z length, w width (px).

// 0 by day, 1 at night: headlights and a glow on the tail lights.
uniform float night = 0.0;

const float BEAM_LENGTH = 1.6; // In vehicle lengths, past the front bumper.
const vec3 GLASS = vec3(0.13, 0.16, 0.2);
const vec3 HEADLAMP = vec3(1.0, 0.96, 0.8);
const vec3 TAIL_LAMP = vec3(0.55, 0.05, 0.05);
const vec3 BRAKE_LAMP = vec3(1.0, 0.12, 0.08);
const vec3 INDICATOR = vec3(1.0, 0.62, 0.05);

varying flat vec4 custom;

void vertex() {
	custom = INSTANCE_CUSTOM;
}

float box(vec2 q, vec2 half_size, float radius) {
	vec2 d = abs(q) - half_size + radius;
	return length(max(d, 0.0)) + min(max(d.x, d.y), 0.0) - radius;
}

// 1 inside the box from (x0, y0) to (x1, y1) in px, antialiased.
float area(vec2 q, float x0, float y0, float x1, float y1, float radius, float aa) {
	vec2 center = vec2(x0 + x1, y0 + y1) * 0.5;
	vec2 half_size = abs(vec2(x1 - x0, y1 - y0)) * 0.5;
	return 1.0 - smoothstep(-aa, aa, box(q - center, half_size, radius));
}

bool flag(int flags, int bit) {
	return (flags & bit) != 0;
}

void fragment() {
	int kind = int(custom.x + 0.5);
	int flags = int(custom.y + 0.5);
	vec2 size = custom.zw;
	vec2 half_size = size * 0.5;
	vec2 q = UV * size;
	float aa = max(fwidth(q.x), 0.001) * 0.75;
	vec3 body = COLOR.rgb;
	bool blink = fract(TIME * 1.5) < 0.5;

	float corner = kind == 0 ? 2.0 : 1.0;
	float inside = area(q, -half_size.x, -half_size.y, half_size.x, half_size.y, corner, aa);
	vec3 color = body;
	float hx = half_size.x;
	float hy = half_size.y;
	if (kind == 0) {
		// Car: bonnet, windscreen, roof, rear window, boot.
		color = mix(color, body * 1.12, area(q, -hx * 0.45, -hy * 0.7, hx * 0.2, hy * 0.7, 1.0, aa));
		color = mix(color, GLASS, area(q, hx * 0.2, -hy * 0.75, hx * 0.5, hy * 0.75, 1.0, aa));
		color = mix(color, GLASS, area(q, -hx * 0.7, -hy * 0.7, -hx * 0.45, hy * 0.7, 0.8, aa));
	} else if (kind == 1) {
		// Truck: cab in front, a gap, then the cargo box with ribs.
		float cab_back = hx - 5.5;
		color = mix(color, body * 0.8, area(q, cab_back, -hy, hx, hy, 1.0, aa));
		color = mix(color, GLASS, area(q, hx - 2.2, -hy * 0.8, hx - 1.0, hy * 0.8, 0.5, aa));
		color = mix(color, vec3(0.12), area(q, cab_back - 1.0, -hy, cab_back, hy, 0.0, aa));
		float box_q = (q.x + hx) / 3.0;
		float rib = step(0.85, fract(box_q)) * step(q.x, cab_back - 1.5);
		color = mix(color, body * 0.85, rib);
		color = mix(color, body * 1.15, area(q, -hx + 0.5, -hy + 0.7, cab_back - 1.5, -hy + 1.3, 0.0, aa));
	} else {
		// Bus: roof panel, air-conditioning unit, windscreen.
		color = mix(color, body * 1.15, area(q, -hx + 1.5, -hy + 1.2, hx - 2.5, hy - 1.2, 0.8, aa));
		color = mix(color, vec3(0.82), area(q, -hx * 0.35, -hy * 0.45, hx * 0.1, hy * 0.45, 0.6, aa));
		color = mix(color, GLASS, area(q, hx - 1.8, -hy + 0.6, hx - 0.4, hy - 0.6, 0.4, aa));
	}

	// Lamps: headlamps and tail lamps at the corners; indicators blink front and back on
	// their side.
	float lamp_x = 1.3;
	float lamp_y = max(hy - 1.6, 0.5);
	float front_lamps = area(q, hx - lamp_x, -hy + 0.3, hx, -lamp_y, 0.3, aa) + area(q, hx - lamp_x, lamp_y, hx, hy - 0.3, 0.3, aa);
	float rear_lamps = area(q, -hx, -hy + 0.3, -hx + lamp_x, -lamp_y, 0.3, aa) + area(q, -hx, lamp_y, -hx + lamp_x, hy - 0.3, 0.3, aa);
	color = mix(color, HEADLAMP, front_lamps * (0.55 + 0.45 * night));
	bool braking = flag(flags, 1);
	color = mix(color, braking ? BRAKE_LAMP : TAIL_LAMP, rear_lamps);
	float glow = 0.0;
	if (blink) {
		float left = flag(flags, 2) ? 1.0 : 0.0;
		float right = flag(flags, 4) ? 1.0 : 0.0;
		float left_lamps = area(q, hx - lamp_x, -hy, hx, -hy + 1.2, 0.3, aa) + area(q, -hx, -hy, -hx + lamp_x, -hy + 1.2, 0.3, aa);
		float right_lamps = area(q, hx - lamp_x, hy - 1.2, hx, hy, 0.3, aa) + area(q, -hx, hy - 1.2, -hx + lamp_x, hy, 0.3, aa);
		color = mix(color, INDICATOR, clamp(left * left_lamps + right * right_lamps, 0.0, 1.0));
	}

	// Outside the body: headlight beams at night, a red glow behind braking vehicles.
	float ahead = q.x - hx;
	float beam = 0.0;
	if (ahead > 0.0 && night > 0.0) {
		float reach = BEAM_LENGTH * size.x;
		float spread = hy * 0.7 + ahead * 0.35;
		beam = night * 0.4 * (1.0 - ahead / reach) * (1.0 - smoothstep(spread * 0.6, spread, abs(q.y)));
		beam = max(beam, 0.0);
	}
	float behind = -hx - q.x;
	if (behind > 0.0 && braking) {
		glow = (0.25 + 0.3 * night) * max(1.0 - behind / 4.0, 0.0) * (1.0 - smoothstep(hy * 0.6, hy * 1.1, abs(q.y)));
	}
	vec4 outside = beam >= glow ? vec4(HEADLAMP, beam) : vec4(BRAKE_LAMP, glow);
	COLOR = mix(outside, vec4(color, COLOR.a), inside);
}
