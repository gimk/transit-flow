# Project Concept: "Transit Flow" (Working Title)

## 1. High-Level Vision & "The Hook"
**The Pitch:** All the satisfaction of untangling massive traffic jams and designing sleek public transit networks in *Cities: Skylines*, completely stripped of the annoying chores like laying water pipes, managing power grids, or balancing tax budgets. 

In this game, you are the **Director of Urban Mobility & Zoning**, not the Mayor. You decide *where* things go and *how* people and goods move between them in a clean, stylized 2D top-down world. The city builds itself around your layout; your only job is to keep the arteries flowing.

---

## 2. Market Inspiration & Reference Games
*   **Mini Motorways / Mini Metro:** Brilliant minimalist 2D focus on pure traffic and transit. *What they lack that you want:* You cannot choose where zones spawn, and there is no separation between freight (trucks) and commuters (cars/buses).
*   **Cities in Motion (1 & 2):** Made by Colossal Order before *Cities: Skylines*. Focused purely on public transit over an existing city. *What it lacks:* You couldn't zone the city or fundamentally redesign the highway networks.
*   **Transport Fever 2:** Incredible logistics (Industrial $\rightarrow$ Commercial) and passenger transport. *What it lacks:* It is macro-scale (connecting separate towns across a massive map) rather than dense, micro-scale urban traffic management.
*   **SimCity 4 (Rush Hour Expansion) / Cities: Skylines (with Traffic Manager: President Edition mod):** The gold standard for the *feeling* you are chasing—watching morning and evening rush hours pulse through your intersections and transit hubs.

---

## 3. Core Gameplay Mechanics (Consolidated)

### A. The 4-Node Ecosystem (Zoning & External Connections)
Instead of managing schools, hospitals, and police, the entire city is boiled down to four color-coded functional elements:
1.  **Residential Zones (Green):** Houses citizens. Generates *Commuters*.
2.  **Commercial Zones (Blue):** Needs *Workers* (from Residential) and *Goods* (from Industrial/External) to operate, and attracts *Shoppers* (from Residential).
3.  **Industrial Zones (Yellow):** Needs *Workers* (from Residential) and *Raw Materials* (from External). Produces *Goods* for Commercial zones or External export.
4.  **External Connections (Map Edges / Ports / Rail Termini):** Fixed entry/exit points on the map edge (Highways, Cargo Train Tracks, Harbors) that supply raw materials, export surplus goods, and bring in new residents.

### B. The Daily Pulse (The 3-Point Agent Loop)
To recreate that satisfying *Cities: Skylines* flow, the game runs on a visual Day/Night cycle where traffic patterns shift dynamically:
*   **06:00 – 10:00 (Morning Rush):** Citizens travel from **Residential $\rightarrow$ Work** (Commercial or Industrial). Heavy directional traffic.
*   **10:00 – 16:00 (Midday Logistics):** Freight trucks dominate the roads, moving goods from **External $\rightarrow$ Industrial $\rightarrow$ Commercial**.
*   **16:00 – 20:00 (Evening Rush & Shopping):** Citizens travel from **Work $\rightarrow$ Commercial (Shopping) $\rightarrow$ Residential**. This is the ultimate stress test for your network, as commuter and commercial traffic collide.
*   **20:00 – 06:00 (Night Owl / Light Freight):** Low traffic; a great visual downtime for the player to pause, analyze heatmaps, and bulldoze/rebuild intersections.

### C. Transport & Road Management Tools
*   **Road Hierarchy:** 2-lane local streets, 4-lane avenues, 6-lane boulevards (with optional dedicated bus lanes), and grade-separated highways/ramps.
*   **Intersection Control:** Toggle stop signs, timed traffic lights, dedicated turning lanes, and roundabouts.
*   **Public Transit:** 
    *   *Buses/Trams:* Share road space (cheap, affected by traffic unless given dedicated lanes).
    *   *Subways/Trains:* Grade-separated (expensive, bypasses road traffic, pulls hundreds of cars off the road).
*   **Freight Separation:** Cargo train terminals and truck-only bypass roads so heavy industrial trucks don't clog up your commercial shopping districts.

---

## 4. How Does the Game Play Without "Money" or "City Chores"?
*   **The Metric of Success: "Flow Efficiency %"**
	Every citizen and cargo truck has a **Patience/Trip Timer**. If they reach their destination quickly, your City Flow score goes up. If they sit in gridlock too long, they despawn (or turn red), and your Flow score drops.
*   **Growth Triggered by Quotas:**
	You don't paint zones at random. A UI bar at the top says: *"City Demands: +50 Residential, +20 Industrial, +30 Commercial."* Once you paint those zones and keep the **Flow Efficiency above 80%** for a full day cycle, the city levels up, granting you "Infrastructure Points" to unlock subways, bigger highways, and expanding the map.
*   **Auto-Upgrading Density:**
    Zones automatically upgrade from low-density (single houses/small shops) to high-density (skyscrapers/megamalls) **solely based on how well-connected they are** to transit and how smoothly traffic flows around them. High density = $10\times$ the traffic challenge!

---

## 5. Technical Architecture in Rust + Bevy (2D Stylized)

(The first version of this document planned the game in Godot 4; it was built that way and then ported to Rust + Bevy. The ideas below are unchanged; the engine words are not.) **Layout:** a Cargo workspace with `tf_sim` (the simulation, pure Rust, no engine) and `tf_game` (the Bevy app: threaded sim runner, views, tools, input). See `CLAUDE.md` and `docs/IMPLEMENTATION_PLAN.md`.

Choosing **2D Top-Down** with a **Grid-now, Curves-later** approach is the smartest possible decision. To avoid rewriting the game when switching from a grid to curves, the underlying data structure treats **a straight grid road as just a 2-point curve**.

### 1. The Secret to "Grid Now, Curves Later": a Polyline Curve Everywhere
Do **not** use a tilemap for vehicle movement! If cars move from tile $(X_1, Y_1)$ to $(X_2, Y_2)$, adding curved roads later means throwing away 100% of the code. Instead, build a **Graph-Based Road System** from Day 1:
*   **Snap to Grid (UI Only):** When the player builds a road, the cursor snaps to a 2D grid (64×64 pixel increments) so building is clean and zones fit inside grid blocks. The sim itself works in world pixels.
*   **Under the Hood (`Curve2D`):** Every lane is stored as a curve (a baked polyline, `geom::Curve2D` / `Polyline` in `tf_sim`) with a start and an end point, even a straight road.
*   **Why this is magical:**
	*   The baked length gives the exact length of the lane, straight or a sweeping S-curve ramp.
	*   Sampling at a distance along the lane gives the exact position and rotation of a car there.
	*   **Rendering:** the baked points go straight into a mesh of the road, so curves work as soon as the player is let off the grid.

### 2. Handling Bridges, Tunnels, and Overpasses in 2D
Every **Intersection Node** and **Road Segment** has an integer property: `elevation_layer` (`-1` = Subway/Tunnel, `0` = Ground, `1` = Elevated Bridge, `2` = High Overpass).
*   Two roads only create an intersection if they cross on the **same `elevation_layer`**.
*   Visually in 2D, the layer sets the draw depth of the road and the cars on it, with a subtle shadow on layer `1`, and layer `-1` shown dashed and translucent in an "Underground View". (The data exists; the drawing is still to do.)

### 3. Pathfinding & Dynamic Traffic Avoidance (A*)
*   Every **lane entry/exit** at an intersection is a node of the lane graph, and every **lane** connecting two of them is a directional edge (`LaneGraph`, with our own A* in `routing/astar.rs`).
*   **Speed & Congestion Weighting:** a lane's weight is recomputed every few seconds from the congestion measured on it:
	$$\text{Segment Weight} = \frac{\text{Curve Length}}{\text{Speed Limit}} \times \left(1 + \text{Congestion Penalty}\right)$$
ight)$$
	When a grid road gets jammed, its weight spikes, and new cars route onto bypasses or take the bus.

### 4. Simulating and Rendering Thousands of 2D Vehicles
*   Vehicles are lightweight data records in the sim (`VehicleSystem`, no ECS, no physics engine): each tick a car advances `distance_along_lane += speed * dt`, and checks the car ahead in the same lane's queue (`Lane.queue`) to keep a safe following distance. Zero collision shapes.
*   The sim runs at a fixed 20 Hz on a **worker thread**, a couple of ticks ahead of the screen. After each tick it captures a `ViewSnapshot`; the render loop interpolates between two snapshots, so frames stay smooth and the sim can't stall them.
*   All cars are drawn as one batched mesh, rewritten in place each frame. At 2 000 vehicles a tick costs about 1.7 ms.

---

## 6. Recommended Step-by-Step Prototype Roadmap (MVP)

1.  **Phase 1: Snapped-Grid `Curve2D` Roads & A Single Car:** 
	*   Create a 2D scene where clicking two grid points builds a road backed by lane curves (one lane going forward, one lane going back).
	*   Place a Green square (Residential) and a Yellow square (Industrial). Spawn a car sprite that queries the router, moves along the lane curves, waits at work, and drives home.
2.  **Phase 2: Intersections & Queuing:** 
	*   When two roads cross on the grid, split their road segments and insert an Intersection Node.
	*   Make cars stop behind the car in front of them, and add a basic 4-way traffic light timer. Watch your first traffic jam form!
3.  **Phase 3: The 3-Zone Loop + External Connection:** 
	*   Add Commercial zones (Blue) and a Map Edge Highway connection. Implement the morning/midday/evening schedule for Citizens vs. Cargo Trucks.
4.  **Phase 4: Unlocking Curves & Bus Lines:** 
	*   Add a toggle to let the player bend the road's control points when drawing a road (instant curved roads!).
	*   Allow the player to click stops on existing roads to create a Bus Route.
