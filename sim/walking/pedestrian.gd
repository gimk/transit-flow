class_name Pedestrian
extends RefCounted
## A person walking a route from door to door. Only PedestrianSystem creates and moves them.

var id: int
## Who the trip belongs to (a citizen id).
var owner_id := -1
var destination_lot_id := -1

## The route: corner points, and per leg (between points i and i+1) the crosswalk it
## crosses (Crosswalks.NONE if none).
var points := PackedVector2Array()
var crossings: Array[Vector2i] = []
var leg := 0
## Distance walked along the current leg.
var leg_progress := 0.0
var position := Vector2.ZERO

## On a crosswalk right now / waiting at its curb (Crosswalks.NONE if not).
var on_crosswalk := Crosswalks.NONE
var waiting_at := Crosswalks.NONE

## Trip timing, like Vehicle's (sim seconds), for the Flow score.
var trip_start_time := 0.0
var ideal_trip_time := 0.0
var trip_end_time := 0.0
