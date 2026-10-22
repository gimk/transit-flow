extends GutTest
## Rounded junction corners (RoadRenderer.corner_fillets): pure geometry from the roads.

const G := 64.0

var roads: RoadNetwork


func before_each() -> void:
	roads = RoadNetwork.new()


func after_each() -> void:
	roads.dispose()


func test_a_crossroads_has_four_rounded_corners_tangent_to_the_kerbs() -> void:
	var local := RoadType.load_by_id(&"local_2lane")
	var avenue := RoadType.load_by_id(&"avenue_4lane")
	roads.add_road(Vector2(0, 4) * G, Vector2(8, 4) * G, avenue)
	roads.add_road(Vector2(4, 0) * G, Vector2(4, 8) * G, local)
	var node := roads.nodes[roads.find_node_at(Vector2(4, 4) * G)]
	var corners := RoadRenderer.corner_fillets(roads, node)
	assert_eq(corners.size(), 4)
	var half_avenue := avenue.get_total_width() * 0.5
	var half_local := local.get_total_width() * 0.5
	for corner in corners:
		var kerb: PackedVector2Array = corner.kerb
		var ends := [kerb[0] - node.position, kerb[-1] - node.position]
		# One end on the avenue's kerb (|y| = its half width), the other on the street's.
		var on_avenue := ends.filter(func(p: Vector2) -> bool: return is_equal_approx(absf(p.y), half_avenue))
		var on_local := ends.filter(func(p: Vector2) -> bool: return is_equal_approx(absf(p.x), half_local))
		assert_eq(on_avenue.size(), 1, "starts on the avenue's kerb")
		assert_eq(on_local.size(), 1, "ends on the street's kerb")
		var tip: Vector2 = (corner.asphalt as PackedVector2Array)[0] - node.position
		assert_almost_eq(absf(tip.x), half_local, 0.01, "fills in from the sharp corner")
		assert_almost_eq(absf(tip.y), half_avenue, 0.01)


func test_only_the_inside_of_a_bend_is_rounded() -> void:
	var local := RoadType.load_by_id(&"local_2lane")
	roads.add_road(Vector2(0, 4) * G, Vector2(4, 4) * G, local)
	roads.add_road(Vector2(4, 4) * G, Vector2(4, 8) * G, local)
	var node := roads.nodes[roads.find_node_at(Vector2(4, 4) * G)]
	assert_eq(RoadRenderer.corner_fillets(roads, node).size(), 1)


func test_no_corner_on_a_straight_road_or_a_dead_end() -> void:
	var local := RoadType.load_by_id(&"local_2lane")
	roads.add_road(Vector2(0, 4) * G, Vector2(4, 4) * G, local)
	roads.add_road(Vector2(4, 4) * G, Vector2(8, 4) * G, local)
	for node: RoadNode in roads.nodes.values():
		assert_eq(RoadRenderer.corner_fillets(roads, node).size(), 0)
