use itertools::{all, enumerate, izip, merge, zip};

use crate::{common::*, rrt::WorldMask};
use crate::nearest_neighbor::*;
use crate::sample_space::*;
use crate::map_io::*;
use crate::prm_graph::*;
use crate::prm_graph;
use bitvec::prelude::*;

pub struct Reachability {
	validity: Vec<WorldMask>,
	reachability: Vec<WorldMask>,
	final_node_ids: Vec<usize>
}

impl Reachability {
	pub fn new() -> Self {
		Self{ validity: Vec::new(), reachability: Vec::new(), final_node_ids: Vec::new() }
	}

	pub fn set_root(&mut self, validity: WorldMask) {
		self.validity.push(validity.clone());
		self.reachability.push(validity);
	}

	pub fn add_node<'a> (&mut self, validity: WorldMask) {
		self.validity.push(validity.clone());
		self.reachability.push(bitvec![0; validity.len()]);
	}

	pub fn add_final_node(&mut self, id: usize) {
		self.final_node_ids.push(id);
	}

	pub fn add_edge(&mut self, from: usize, to: usize) {
		let mut tmp = self.reachability[from].clone();
		tmp &= self.validity[to].clone();
		self.reachability[to] |= tmp;
	}

	pub fn reachability(&self, id: usize) -> &WorldMask {
		&self.reachability[id]
	}

	pub fn final_nodes_for_world(&self, world: usize) -> Vec<usize> {
		self.final_node_ids.iter()
			.filter(|&id| self.reachability[*id][world])
			.map(|&id| id)
			.collect()
	}

	pub fn is_final_set_complete(&self) -> bool {
		if self.final_node_ids.is_empty() { return false; }

		// get first elements as starting point..
		let &first_final_id = self.final_node_ids.first().unwrap();
		let first_reachability = self.reachability[first_final_id].clone();

		self.final_node_ids.iter().skip(0)
			.fold(first_reachability, |reachability, &id| reachability | self.reachability[id].clone())
			.all()
	}
}

pub struct PRM<'a, F: PRMFuncs<N>, const N: usize> {
	continuous_sampler: ContinuousSampler<N>,
	discrete_sampler: DiscreteSampler,
	fns: &'a F,
	graph: PRMGraph<N>,
	conservative_reachability: Reachability,
	n_it: usize
}

impl<'a, F: PRMFuncs<N>, const N: usize> PRM<'a, F, N> {
	pub fn new(continuous_sampler: ContinuousSampler<N>, discrete_sampler: DiscreteSampler, fns: &'a F) -> Self {
		Self { continuous_sampler, discrete_sampler, fns, graph: PRMGraph{nodes: vec![]}, conservative_reachability: Reachability::new(), n_it: 0 }
	}

	pub fn grow_graph(&mut self, &start: &[f64; N], goal: fn(&[f64; N]) -> bool,
				max_step: f64, search_radius: f64, n_iter_min: usize, n_iter_max: usize) -> bool {

		let root_validity = self.fns.state_validity(&start).expect("Start from a valid state!");
		let n_worlds = root_validity.len();
		self.graph.add_node(start, root_validity.clone());
		self.conservative_reachability.set_root(root_validity);
		let mut kdtree = KdTree::new(start);

		let mut i = 0;
		while i < n_iter_min || !self.conservative_reachability.is_final_set_complete() && i < n_iter_max {
			i+=1;
	
			// First sample state and world
			let mut new_state = self.continuous_sampler.sample();
			let world = self.discrete_sampler.sample(n_worlds);

			// Second, retrieve closest node for sampled world and steer from there
			//let kd_from = kdtree.nearest_neighbor(new_state); // n log n
			let kd_from = kdtree.nearest_neighbor_filtered(new_state, &|id|{self.conservative_reachability.reachability(id)[world]}); // n log n
			steer(&kd_from.state, &mut new_state, max_step); 

			let state_validity = self.fns.state_validity(&new_state);
			if state_validity.is_some() {
				// Third, add node
				let new_node_id = self.graph.add_node(new_state, state_validity.clone().unwrap());
				let new_node = &self.graph.nodes[new_node_id];
				self.conservative_reachability.add_node(state_validity.unwrap());

				// Fourth, we find the neighbors in a specific radius of new_state.
				let radius = {
					let n = self.graph.nodes.len() as f64;
					let s = search_radius * (n.ln()/n).powf(1.0/(N as f64));
					if s < max_step { s } else { max_step }
				};

				kdtree.add(new_state, new_node_id);

				// Fifth we connect to neighbors 
				let neighbour_ids: Vec<usize> = kdtree.nearest_neighbors(new_state, radius).iter()
				.map(|&kd_node| kd_node.id)
				.collect();

				let fwd_ids: Vec<usize> = neighbour_ids.iter()
					.map(|&id| (id, &self.graph.nodes[id]))
					.filter(|(_, node)| self.fns.transition_validator(node, new_node))
					.map(|(id, _)| id)
					.collect();

				/*let bwd_ids: Vec<usize> = neighbour_ids.iter()
				.map(|&id| (id, &self.graph.nodes[id]))
				.filter(|(_, node)| self.fns.transition_validator(new_node, node))
				.map(|(id, _)| id.clone())
				.collect();*/
							
				for &id in &fwd_ids {
					self.graph.add_edge(id, new_node_id);
					self.conservative_reachability.add_edge(id, new_node_id);
				}
				
				/*for &id in &bwd_ids {
					self.graph.add_edge(new_node_id, id);
					self.conservative_reachability.add_edge(new_node_id, id);
				}*/

				if goal(&new_state) {
					self.conservative_reachability.add_final_node(new_node_id);
				}
			}
		}

		self.n_it += i;

		self.conservative_reachability.is_final_set_complete()
	}

	pub fn plan(&mut self, _: &[f64; N], _: &Vec<f64>) -> Result<Vec<[f64; N]>, &'static str> {
		let final_nodes_in_0 = self.conservative_reachability.final_nodes_for_world(0);
		let _dist_to_final = dijkstra(&self.graph, &final_nodes_in_0, 0, self.fns);

		Err("")
	}

	pub fn print_summary(&self) {
		println!("number of iterations:{}", self.n_it);
		self.graph.print_summary();
	}
}

#[cfg(test)]
mod tests {

use super::*;

#[test]
fn test_plan_on_map() {
	let mut m = Map::open("data/map2.pgm", [-1.0, -1.0], [1.0, 1.0]);
	m.add_zones("data/map2_zone_ids.pgm");

	fn goal(state: &[f64; 2]) -> bool {
		(state[0] - 0.0).abs() < 0.05 && (state[1] - 0.9).abs() < 0.05
	}

	let mut prm = PRM::new(ContinuousSampler::new([-1.0, -1.0], [1.0, 1.0]),
						   DiscreteSampler::new(),
						   &m);

	prm.grow_graph(&[0.55, -0.8], goal, 0.05, 5.0, 1000, 10000);

	prm.print_summary();
	
	let _ = prm.plan(&[0.0, -0.8], &vec![0.25, 0.25, 0.25, 0.25]);

	// loop:
	// prm.plan(position, prior); 	// potentiallement adapter graph si on arrive dans un monde improbable lors du precompute

	let world = 0; 
	let mut full = m.clone();
	full.set_world(world);
	full.draw_full_graph(&prm.graph);
	//full.draw_graph_for_world(&prm.graph, world);
	full.save("results/test_prm_graph.pgm");
}

#[test]
fn test_reachability() {
	/*
		0
		|
		1
	   / \
	  2   3
	*/
	let mut reachability = Reachability::new();

	reachability.set_root(bitvec![1,1]); // 0
	reachability.add_node(bitvec![1,0]); // 1
	reachability.add_node(bitvec![1,0]); // 2
	reachability.add_node(bitvec![0,1]); // 3

	reachability.add_edge(0, 1);
	reachability.add_edge(1, 2);
	reachability.add_edge(1, 3);

	assert_eq!(reachability.reachability(0), &bitvec![1,1]);
	assert_eq!(reachability.reachability(1), &bitvec![1,0]);
	assert_eq!(reachability.reachability(2), &bitvec![1,0]);
	assert_eq!(reachability.reachability(3), &bitvec![0,0]);
}

#[test]
fn test_reachability_diamond_shape() {
	/*
		0
	   / \
	  1   2
	   \ /
	    3
	*/
	let mut reachability = Reachability::new();

	reachability.set_root(bitvec![1,1]); // 0
	reachability.add_node(bitvec![1,0]); // 1
	reachability.add_node(bitvec![0,1]); // 2
	reachability.add_node(bitvec![1,1]); // 3

	reachability.add_edge(0, 1);
	reachability.add_edge(0, 2);
	reachability.add_edge(1, 3);
	reachability.add_edge(2, 3);

	assert_eq!(reachability.reachability(0), &bitvec![1,1]);
	assert_eq!(reachability.reachability(1), &bitvec![1,0]);
	assert_eq!(reachability.reachability(2), &bitvec![0,1]);
	assert_eq!(reachability.reachability(3), &bitvec![1,1]);
}

#[test]
fn test_final_nodes_completness() {
	/*
		0
		|
		1
	   / \
	  2   3
	*/
	let mut reachability = Reachability::new();

	reachability.set_root(bitvec![1,1]); // 0
	reachability.add_node(bitvec![1,1]); // 1
	reachability.add_node(bitvec![1,0]); // 2
	reachability.add_node(bitvec![0,1]); // 3

	reachability.add_edge(0, 1);
	reachability.add_edge(1, 2);
	reachability.add_edge(1, 3);

	assert_eq!(reachability.is_final_set_complete(), false);

	reachability.add_final_node(2);
	assert_eq!(reachability.is_final_set_complete(), false);

	reachability.add_final_node(3);
	assert_eq!(reachability.is_final_set_complete(), true);

	assert_eq!(reachability.final_nodes_for_world(0), vec![2]);
	assert_eq!(reachability.final_nodes_for_world(1), vec![3]);
}

}

/* N = nombre de mondes
* PLAN()
* Option 1/ Pour chaque monde, calculer la distance à l'objectif de chaque node (Value iteration)
* 	-> avantage : valeurs valables partout
*   -> long calcul offline - scalability?
*   d(n) = min( d(m, n) + d(m) for m in children(n) )
*
* Option 2/ Au runtime, calculer meilleur chemin pour chacun des mondes (e.g. A*) et prendre decision en pondérant chacunes des options 
*	-> avantage : étape offline moins longue
*   -> possible à l'exécution ? (assez rapide?)
*
* Option 3/ Monte Carlo -> N
* 
* Option 4/ N mondes: RRT start N fois
* 
*
* EXECUTION()
* - Robot a un belief state : probabilité d'être dans chaque monde
* - Deduit le chemin a suivre en cherchant en calculant l'espérance de la distance à l'objectif de chacun de ces enfants
*
* Belief state : [0.25, 0.25, 0.25, 0.25] 
* 
*
* QUESTIONS:
* - Quand arreter de faire croitre le graph et quand même avoir une solution our chaque monde??
*/

// Compresser pour avoir N mondes même pour des domaines où le nombre de mondes explose

// Done:
// - random seed + time saving
// - dijkstra
// - more efficient tree growing
// - reachability
// - serializaion
