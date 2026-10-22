extends GutTest
## Greenery on unzoned land (NatureRenderer.lay_out): trees and parks keep off roads,
## sidewalks, zones and lots, and come out the same every time.

const G := 64.0

var sim: Simulation


func before_each() -> void:
	sim = Simulation.new()
	var local := RoadType.load_by_id(&"local_2lane")
	sim.roads.add_road(Vector2(10, 20) * G, Vector2(40, 20) * G, RoadType.load_by_id(&"avenue_4lane"))
	sim.roads.add_road(Vector2(25, 5) * G, Vector2(25, 35) * G, local)
	sim.roads.add_curved_road(Vector2(30, 30) * G, Vector2(36, 30) * G, Vector2(36, 36) * G, local)
	var cells: Array[Vector2i] = []
	for x in range(20, 25):
		cells.append(Vector2i(x, 20))
	sim.zones.paint(cells, Lot.Type.RESIDENTIAL)
	sim.lots.add_lot(Vector2i(27, 19), Lot.Type.COMMERCIAL)


func after_each() -> void:
	sim.dispose()


func test_greenery_keeps_off_roads_sidewalks_zones_and_lots() -> void:
	var started := Time.get_ticks_msec()
	var layout := NatureRenderer.lay_out(sim.roads, sim.zones.zones, sim.lots)
	gut.p("laid out the whole map in %d ms" % (Time.get_ticks_msec() - started))
	var trees: Array[Vector3] = layout.trees
	assert_gt(trees.size(), 100, "a fair number of trees")
	assert_gt((layout.parks as Array).size(), 0, "some parks beside the roads")
	for tree in trees:
		var at := Vector2(tree.x, tree.y)
		var cell := Grid.world_to_cell(at)
		assert_false(sim.zones.zones.has(cell), "not on zoned land")
		assert_eq(sim.lots.get_lot_at_cell(cell), -1, "not on a lot")
		for segment: RoadSegment in sim.roads.segments.values():
			var points := LaneBuilder.get_polyline(segment.centerline)
			var clear := segment.road_type.get_total_width() * 0.5 + RoadRenderer.SIDEWALK_WIDTH
			for i in points.size() - 1:
				var gap := at.distance_to(Geometry2D.get_closest_point_to_segment(at, points[i], points[i + 1]))
				if gap < clear + tree.z - 1.0:
					fail_test("tree at %s over the road or its sidewalk" % at)
					return
	for park: Dictionary in layout.parks:
		assert_false(sim.zones.zones.has(Grid.world_to_cell((park.plot as Rect2).get_center())))


func test_the_same_map_gets_the_same_greenery() -> void:
	var first := NatureRenderer.lay_out(sim.roads, sim.zones.zones, sim.lots)
	var second := NatureRenderer.lay_out(sim.roads, sim.zones.zones, sim.lots)
	assert_eq(first.trees, second.trees)
