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

## 5. Technical Architecture in Godot 4.x (2D Stylized)

Choosing **2D Top-Down** with a **Grid-now, Curves-later** approach is the smartest possible decision. However, to make sure you don't have to rewrite your entire game when you switch from a Grid to Bezier Curves later, we must design the underlying data structure so that **a straight grid road is treated as just a 2-point curve**.

### 1. The Secret to "Grid Now, Curves Later": `Curve2D` Everywhere
Do **not** use Godot's `TileMapLayer` for your vehicle movement logic! If you program cars to move from tile $(X_1, Y_1)$ to $(X_2, Y_2)$, adding curved roads later will require throwing away 100% of your code.
Instead, build a **Graph-Based Road System** from Day 1:
*   **Snap to Grid (UI Only):** When the player clicks to build a road in Phase 1, snap their mouse cursor to a 2D grid (e.g., $64\times64$ pixel increments) so building is clean and easy, and zones fit nicely inside grid blocks.
*   **Under the Hood (`Curve2D`):** Even if the player draws a straight horizontal road on the grid, store that road lane in memory as a `Curve2D` with a Start Point and an End Point.
*   **Why this is magical:** 
	*   `Curve2D.get_baked_length()` gives you the exact length of the lane (whether it's a straight grid line now, or a sweeping S-curve highway ramp later).
	*   `Curve2D.sample_baked_with_rotation(distance_in_pixels)` gives you the exact `Transform2D` (X/Y position and rotation angle) of a car at any distance along that lane!
	*   **Rendering:** You can pass `Curve2D.get_baked_points()` directly into a `Line2D` node with a repeating road texture. In 10 lines of code, you get crisp 2D roads that automatically support curves whenever you are ready to unlock the mouse from the grid!

### 2. Handling Bridges, Tunnels, and Overpasses in 2D
Since you want highway interchanges and subways in a 2D game, how do two roads cross without creating an intersection?
*   Every **Intersection Node** and **Road Segment** has an integer property: `elevation_layer` (`-1` = Subway/Tunnel, `0` = Ground, `1` = Elevated Bridge, `2` = High Overpass).
*   Two roads only create an intersection if they cross on the **same `elevation_layer`**.
*   Visually in 2D, you just set the `z_index` of the `Line2D` and cars to match their `elevation_layer`, add a subtle drop-shadow shader to Layer `1`, and render Layer `-1` as a semi-transparent dashed line when the player opens the "Underground View"!

### 3. Pathfinding & Dynamic Traffic Avoidance (`AStar2D`)
*   Every **Lane Entry/Exit** at an intersection is a point in `AStar2D`.
*   Every **Lane** (`Curve2D`) connecting two intersections is a directional segment in `AStar2D`.
*   **Speed & Congestion Weighting:** Curved highway ramps will naturally be shorter or have higher speed limits ($V_{\text{max}}$) than sharp $90^\circ$ grid turns. You can calculate the `AStar2D` weight of a road segment dynamically every few seconds:
	$$\text{Segment Weight} = \frac{\text{Curve Length}}{\text{Speed Limit}} \times \left(1 + \text{Congestion Penalty}\right)$$
	When a grid road gets jammed, its weight spikes, and new cars will automatically route onto your bypass highways or take the bus!

### 4. Rendering Thousands of 2D Vehicles (`MultiMeshInstance2D`)
*   Store all vehicles in a pure GDScript/C# array of lightweight data objects.
*   Each frame, a car simply increases its `distance_along_curve += current_speed * delta`.
*   To prevent cars from driving through each other, a car just checks the `distance_along_curve` of the car directly ahead of it in the same lane array. If $\Delta d < \text{safe\_following\_distance}$, it slows down or stops. Zero 2D physics or `Area2D` collision nodes needed!
*   Draw all cars in a single batch using `MultiMeshInstance2D` (using `set_instance_transform_2d` and `set_instance_color`).

---

## 6. Recommended Step-by-Step Prototype Roadmap (MVP)

1.  **Phase 1: Snapped-Grid `Curve2D` Roads & A Single Car:** 
	*   Create a 2D scene where clicking two grid points spawns a `Line2D` backed by a `Curve2D` (one lane going forward, one lane going back).
	*   Place a Green square (Residential) and a Yellow square (Industrial). Spawn a car sprite that queries `AStar2D`, moves along the `Curve2D` lanes using `sample_baked_with_rotation()`, waits at work, and drives home.
2.  **Phase 2: Intersections & Queuing:** 
	*   When two roads cross on the grid, split their `Curve2D` segments and insert an Intersection Node.
	*   Make cars stop behind the car in front of them, and add a basic 4-way traffic light timer. Watch your first traffic jam form!
3.  **Phase 3: The 3-Zone Loop + External Connection:** 
	*   Add Commercial zones (Blue) and a Map Edge Highway connection. Implement the morning/midday/evening schedule for Citizens vs. Cargo Trucks.
4.  **Phase 4: Unlocking Curves & Bus Lines:** 
	*   Add a toggle to let the player bend the `Curve2D` control points when drawing a road (instant curved roads!).
	*   Allow the player to click stops on existing roads to create a Bus Route.