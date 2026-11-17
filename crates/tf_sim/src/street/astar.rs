//! A* search over dense point indices, used by the walkers' area graph (`street::run::walk`)
//! to find the cheapest chain of areas. It is Godot 4.7's `AStar2D` search
//! (`AStar2D::_solve`), kept as it was so that routes, and with them the replay hashes,
//! do not change: ties between equally cheap routes are broken as Godot breaks them:
//! neighbours are tried in the order they were connected, the open list is Godot's binary
//! heap (`SortArray` `push_heap` / `adjust_heap` / `pop_heap`), points are ordered by `f`
//! then by the larger `g`, and scores are `real_t` (`f32`). Points are dense indices (the
//! order they were added in); callers map their own ids onto them. The step cost and the
//! estimate come from the caller, as `f32`.

use glam::Vec2;

#[derive(Clone, Debug, Default)]
pub struct AStar {
    positions: Vec<Vec2>,
    /// Outgoing connections per point, in the order they were connected.
    neighbors: Vec<Vec<usize>>,
    // Search state, reused between searches (Godot's per-point pass markers).
    pass: u64,
    open_pass: Vec<u64>,
    closed_pass: Vec<u64>,
    g_score: Vec<f32>,
    f_score: Vec<f32>,
    prev: Vec<usize>,
}

impl AStar {
    pub fn new() -> Self {
        Self::default()
    }

    /// `clear()`.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn point_count(&self) -> usize {
        self.positions.len()
    }

    /// `add_point`: returns the new point's index.
    pub fn add_point(&mut self, position: Vec2) -> usize {
        self.positions.push(position);
        self.neighbors.push(Vec::new());
        self.open_pass.push(0);
        self.closed_pass.push(0);
        self.g_score.push(0.0);
        self.f_score.push(0.0);
        self.prev.push(usize::MAX);
        self.positions.len() - 1
    }

    pub fn position(&self, point: usize) -> Vec2 {
        self.positions[point]
    }

    /// The points `point` leads to, in connection order (`get_point_connections`).
    pub fn connections(&self, point: usize) -> &[usize] {
        &self.neighbors[point]
    }

    /// `connect_points(from, to, false)`: a one-way connection.
    pub fn connect_one_way(&mut self, from: usize, to: usize) {
        if !self.neighbors[from].contains(&to) {
            self.neighbors[from].push(to);
        }
    }

    /// `connect_points(a, b)`: both ways (a point can't connect to itself).
    pub fn connect_points(&mut self, a: usize, b: usize) {
        if a == b {
            return;
        }
        self.connect_one_way(a, b);
        self.connect_one_way(b, a);
    }

    /// `are_points_connected(from, to, false)`.
    pub fn are_connected(&self, from: usize, to: usize) -> bool {
        self.neighbors[from].contains(&to)
    }

    /// `get_id_path(from, to)`: the points of the cheapest path, both ends included;
    /// empty if there is none. `cost(positions, from, to)` is the price of one step and
    /// `estimate(from position, end position)` an underestimate of what is left (A*'s heuristic).
    pub fn id_path(
        &mut self,
        begin: usize,
        end: usize,
        cost: impl FnMut(&[Vec2], usize, usize) -> f32,
        estimate: impl Fn(Vec2, Vec2) -> f32,
    ) -> Vec<usize> {
        if begin == end {
            return vec![begin];
        }
        if !self.solve(begin, end, cost, estimate) {
            return Vec::new();
        }
        let mut path = vec![end];
        let mut point = end;
        while point != begin {
            point = self.prev[point];
            path.push(point);
        }
        path.reverse();
        path
    }

    /// `SortPoints`: true when point `a` is worse than point `b` (higher f; on equal f,
    /// lower g: points further from the start go first).
    fn worse(&self, a: usize, b: usize) -> bool {
        let (fa, fb) = (self.f_score[a], self.f_score[b]);
        if fa > fb {
            true
        } else if fa < fb {
            false
        } else {
            self.g_score[a] < self.g_score[b]
        }
    }

    /// `SortArray::push_heap`.
    fn push_heap(&self, heap: &mut [usize], mut hole: usize, top: usize, value: usize) {
        while hole > top {
            let parent = (hole - 1) / 2;
            if !self.worse(heap[parent], value) {
                break;
            }
            heap[hole] = heap[parent];
            hole = parent;
        }
        heap[hole] = value;
    }

    /// `SortArray::adjust_heap`.
    fn adjust_heap(&self, heap: &mut [usize], mut hole: usize, len: usize, value: usize) {
        let top = hole;
        let mut second_child = 2 * hole + 2;
        while second_child < len {
            if self.worse(heap[second_child], heap[second_child - 1]) {
                second_child -= 1;
            }
            heap[hole] = heap[second_child];
            hole = second_child;
            second_child = 2 * (second_child + 1);
        }
        if second_child == len {
            heap[hole] = heap[second_child - 1];
            hole = second_child - 1;
        }
        self.push_heap(heap, hole, top, value);
    }

    /// `SortArray::pop_heap(0, len)`: moves the best point to the end.
    fn pop_heap(&self, heap: &mut [usize]) {
        let last = heap.len() - 1;
        let value = heap[last];
        heap[last] = heap[0];
        self.adjust_heap(heap, 0, last, value);
    }

    /// `AStar2D::_solve`.
    fn solve(
        &mut self,
        begin: usize,
        end: usize,
        mut cost: impl FnMut(&[Vec2], usize, usize) -> f32,
        estimate: impl Fn(Vec2, Vec2) -> f32,
    ) -> bool {
        self.pass += 1;
        let pass = self.pass;
        let mut open: Vec<usize> = Vec::new();
        self.g_score[begin] = 0.0;
        self.f_score[begin] = estimate(self.positions[begin], self.positions[end]);
        open.push(begin);
        while let Some(&point) = open.first() {
            if point == end {
                return true;
            }
            self.pop_heap(&mut open);
            open.pop();
            self.closed_pass[point] = pass;
            for k in 0..self.neighbors[point].len() {
                let neighbor = self.neighbors[point][k];
                if self.closed_pass[neighbor] == pass {
                    continue;
                }
                let tentative = self.g_score[point] + cost(&self.positions, point, neighbor) * 1.0;
                let new_point = self.open_pass[neighbor] != pass;
                if new_point {
                    self.open_pass[neighbor] = pass;
                    open.push(neighbor);
                } else if tentative >= self.g_score[neighbor] {
                    continue;
                }
                self.prev[neighbor] = point;
                self.g_score[neighbor] = tentative;
                self.f_score[neighbor] =
                    tentative + estimate(self.positions[neighbor], self.positions[end]);
                let hole = if new_point {
                    open.len() - 1
                } else {
                    open.iter()
                        .position(|&p| p == neighbor)
                        .expect("in the open list")
                };
                self.push_heap(&mut open, hole, 0, neighbor);
            }
        }
        false
    }
}
