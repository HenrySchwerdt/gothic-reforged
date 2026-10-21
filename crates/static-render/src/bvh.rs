//! Cluster BVH over static instances. Leaves are clusters of up to `leaf_size` nearby
//! instances (the unit the renderer toggles); inner nodes let a query reject or accept whole
//! subtrees at once.
use crate::geom::{Aabb, Containment, Frustum};
use glam::Vec3;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Leaf { cluster: u32 },
    Inner { left: u32, right: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Node {
    bounds: Aabb,
    kind: Kind,
}

/// A cluster: a contiguous run of `ClusterBvh::order`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cluster {
    pub bounds: Aabb,
    pub start: u32,
    pub count: u32,
}

#[derive(Debug, Clone, Default)]
pub struct ClusterBvh {
    nodes: Vec<Node>,
    pub clusters: Vec<Cluster>,
    /// Instance indices grouped by cluster.
    pub order: Vec<u32>,
    /// Cluster of each instance.
    pub cluster_of: Vec<u32>,
}

/// Visibility query input.
#[derive(Debug, Clone)]
pub struct CullQuery<'a> {
    pub frustum: &'a Frustum,
    pub eye: Vec3,
    /// Clusters farther than this (box distance) are culled.
    pub max_distance: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CullStats {
    pub nodes_visited: u32,
    pub clusters_visible: u32,
    pub clusters_inside: u32,
}

impl ClusterBvh {
    /// Build over instance bounds (median split along the longest axis of the centroids).
    pub fn build(bounds: &[Aabb], leaf_size: usize) -> Self {
        let leaf_size = leaf_size.max(1);
        let mut order: Vec<u32> = (0..bounds.len() as u32).collect();
        let mut bvh = Self {
            nodes: Vec::new(),
            clusters: Vec::new(),
            order: Vec::new(),
            cluster_of: vec![0; bounds.len()],
        };
        if !bounds.is_empty() {
            let n = order.len();
            bvh.build_node(bounds, &mut order, 0, n, leaf_size);
        }
        for (ci, c) in bvh.clusters.iter().enumerate() {
            for &i in &order[c.start as usize..(c.start + c.count) as usize] {
                bvh.cluster_of[i as usize] = ci as u32;
            }
        }
        bvh.order = order;
        bvh
    }

    fn build_node(
        &mut self,
        bounds: &[Aabb],
        order: &mut [u32],
        start: usize,
        end: usize,
        leaf: usize,
    ) -> u32 {
        let slice = &mut order[start..end];
        let total = slice
            .iter()
            .fold(Aabb::EMPTY, |a, &i| a.union(&bounds[i as usize]));
        let id = self.nodes.len() as u32;
        self.nodes.push(Node {
            bounds: total,
            kind: Kind::Leaf { cluster: 0 },
        });
        if slice.len() <= leaf {
            let cluster = self.clusters.len() as u32;
            self.clusters.push(Cluster {
                bounds: total,
                start: start as u32,
                count: slice.len() as u32,
            });
            self.nodes[id as usize].kind = Kind::Leaf { cluster };
            return id;
        }
        let cb = slice.iter().fold(Aabb::EMPTY, |a, &i| {
            let c = bounds[i as usize].center();
            a.union(&Aabb::new(c, c))
        });
        let ext = cb.max - cb.min;
        let axis = if ext.x >= ext.y && ext.x >= ext.z {
            0
        } else if ext.y >= ext.z {
            1
        } else {
            2
        };
        let mid = slice.len() / 2;
        slice.select_nth_unstable_by(mid, |&a, &b| {
            let ca = bounds[a as usize].center()[axis];
            let cb = bounds[b as usize].center()[axis];
            ca.total_cmp(&cb).then(a.cmp(&b))
        });
        let left = self.build_node(bounds, order, start, start + mid, leaf);
        let right = self.build_node(bounds, order, start + mid, end, leaf);
        self.nodes[id as usize].kind = Kind::Inner { left, right };
        id
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Mark visible clusters in `visible` (indexed by cluster, OR-ed in so several queries,
    /// e.g. the view and each shadow cascade, can share one mask).
    pub fn cull(&self, q: &CullQuery, visible: &mut [bool]) -> CullStats {
        let mut stats = CullStats::default();
        if self.nodes.is_empty() {
            return stats;
        }
        let mut stack: Vec<(u32, bool)> = vec![(0, false)];
        while let Some((n, inside)) = stack.pop() {
            stats.nodes_visited += 1;
            let node = &self.nodes[n as usize];
            if node.bounds.distance(q.eye) > q.max_distance {
                continue;
            }
            let mut fully = inside;
            if !inside {
                match q.frustum.test_aabb(&node.bounds) {
                    Containment::Outside => continue,
                    Containment::Inside => fully = true,
                    Containment::Intersects => {}
                }
            }
            match node.kind {
                Kind::Leaf { cluster } => {
                    stats.clusters_visible += 1;
                    stats.clusters_inside += u32::from(fully);
                    visible[cluster as usize] = true;
                }
                Kind::Inner { left, right } => {
                    stack.push((right, fully));
                    stack.push((left, fully));
                }
            }
        }
        stats
    }

    /// Instances of a cluster.
    pub fn members(&self, cluster: usize) -> &[u32] {
        let c = &self.clusters[cluster];
        &self.order[c.start as usize..(c.start + c.count) as usize]
    }
}
