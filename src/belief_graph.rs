use itertools::{all, enumerate, izip, merge, zip};

use crate::common::*;
use crate::nearest_neighbor::*;
use crate::sample_space::*;
use crate::map_io::*; // tests only
use bitvec::prelude::*;
use priority_queue::PriorityQueue;
use std::{collections::BTreeMap, ops::Index};

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum BeliefNodeType {
    Unknown,
    Action,
    Observation,
}

pub struct BeliefNode<const N: usize> {
	pub state: [f64; N],
    pub belief_state: BeliefState,
    pub belief_id: usize,
	pub parents: Vec<usize>,
    pub children: Vec<usize>,
    pub node_type: BeliefNodeType,
}

pub struct BeliefGraph<const N: usize> {
    pub nodes: Vec<BeliefNode<N>>,
    pub reachable_belief_states: Vec<Vec<f64>>
}

impl<const N: usize> BeliefGraph<N> {
	pub fn add_node(&mut self, state: [f64; N], belief_state: BeliefState, belief_id: usize, node_type: BeliefNodeType) -> usize {
        let id = self.nodes.len();
        self.nodes.push(
            BeliefNode{
                state,
                belief_state,
                belief_id,
                parents: Vec::new(),
                children: Vec::new(),
                node_type,
            }
        );
        id
    }

	pub fn add_edge(&mut self, from_id: usize, to_id: usize) {
		self.nodes[from_id].children.push(to_id);
		self.nodes[to_id].parents.push(from_id);
    }
    
    #[allow(clippy::style)]
    pub fn belief_id(&self, belief_state: &BeliefState) -> usize {
        self.reachable_belief_states.iter().position(|belief| belief == belief_state).expect("belief state should be found here") // TODO: improve
    }
}

#[allow(clippy::style)]
pub fn transition_probability(parent_bs: &BeliefState, child_bs: &BeliefState) -> f64 {
    child_bs.iter().zip(parent_bs).fold(0.0, |s, (p, q)| s + if *p > 0.0 { *q } else { 0.0 } )
}

pub fn conditional_dijkstra<const N: usize>(graph: &BeliefGraph<N>, final_node_ids: &[usize], cost_evaluator: impl Fn(&[f64; N], &[f64; N]) -> f64) -> Vec<f64> {
	// https://fr.wikipedia.org/wiki/Algorithme_de_Dijkstra
	// complexité n log n ;graph.nodes.len()
    let mut dist = vec![std::f64::INFINITY; graph.nodes.len()];
	let mut q = PriorityQueue::new();
    
    // debug
    println!("number of belief nodes:{}", graph.nodes.len());
    // 

	for &id in final_node_ids {
		dist[id] = 0.0;
        q.push(id, Priority{prio: 0.0});
	}

    let mut it = 0;
	while !q.is_empty() {
        it+=1;
        let (v_id, _) = q.pop().unwrap();
        
        // debug
        if it % 10000 == 0 {
            println!("number of iterations:{}", it);
            println!("queue size:{}, v_id:{}", q.len(), v_id);
        }
        //
		for &u_id in &graph.nodes[v_id].parents {
            let u = &graph.nodes[u_id];

            let mut alternative = 0.0;
            if u.node_type == BeliefNodeType::Action {
                let v = &graph.nodes[v_id];
                alternative += cost_evaluator(&u.state, &v.state) + dist[v_id]
            }
            else if u.node_type == BeliefNodeType::Observation {
                for &vv_id in &u.children {
                    let vv = &graph.nodes[vv_id];
                    let p = transition_probability(&graph.nodes[u_id].belief_state, &graph.nodes[vv_id].belief_state);

                    //println!("belief avant:{:?} apres:{:?}", graph.belief_state(u_id), graph.belief_state(vv_id));
                    //assert_eq!(u.children().len(), 2);

                    alternative += p * (cost_evaluator(&u.state, &vv.state) + dist[vv_id]);
                }

                //println!("alternative for : {} = {}", u_id, alternative);
            }
            else {
                panic!("node type should be know at this stage!");
            }

			if alternative < dist[u_id] {
                dist[u_id] = alternative;
                q.push(u_id, Priority{prio: alternative});
            }
		}
    }

    // checks 
    /*
    for id in 0..graph.n_nodes() {
        let n = graph.node(id);

        if *n.node_type() == BeliefNodeType::Observation {
            println!("belief: {:?}, cost:{}", graph.belief_state(id), dist[id]);
        }

        if dist[id] < 5.0 && !final_node_ids.contains(&id) {
            assert!(n.children().len() > 0);
            if n.children().len() == 0 {
                println!("pb!!!, node_type:{:?}", n.node_type());
            }
        }

        for child_id in n.children() {
            let o = graph.node(*child_id);

            assert!(o.parents().contains(&id));

            if ! o.parents().contains(&id) {
                println!("pb!!!, node_type:{:?}", n.node_type());
            }
        }
    }
    */
    // debug
    println!("conditional dijkstra finished..");
    // 

	dist
}

pub fn extract_policy<const N: usize>(graph: &BeliefGraph<N>, expected_costs_to_goals: &[f64]) -> Policy<N> {
    if graph.nodes.is_empty() {
        panic!("no belief state graph!");
    }

    let mut policy: Policy<N> = Policy{nodes: Vec::new(), leafs: Vec::new()};
    let mut lifo: Vec<(usize, usize)> = Vec::new(); // policy_node, belief_graph_node

    policy.add_node(&graph.nodes[0].state, &graph.nodes[0].belief_state, false);

    lifo.push((0, 0));

    while !lifo.is_empty() {
        let (policy_node_id, belief_node_id) = lifo.pop().unwrap();

        let children_ids = get_best_expected_children(graph, belief_node_id, expected_costs_to_goals);

        for child_id in children_ids {
            let child = &graph.nodes[child_id];
            let is_leaf = expected_costs_to_goals[child_id] == 0.0;
            let child_policy_id = policy.add_node(&child.state, &graph.nodes[child_id].belief_state, is_leaf);
            policy.add_edge(policy_node_id, child_policy_id);

            //println!("add node, belief {:?}, cost: {:?}", &graph.belief_state(child_id), &expected_costs_to_goals[child_id]);

            if ! is_leaf {
                lifo.push((child_policy_id, child_id));
            }
        }
    }
    policy
}

pub fn get_best_expected_children<const N: usize>(graph: &BeliefGraph<N>, belief_node_id: usize, expected_costs_to_goals: &[f64]) -> Vec<usize> {    
    // cluster children by target belief state
    let mut belief_to_children = BTreeMap::new();
    for &child_id in &graph.nodes[belief_node_id].children {
        let child = &graph.nodes[child_id];

        belief_to_children.entry(child.belief_id).or_insert_with(Vec::new);
        belief_to_children.get_mut(&child.belief_id).unwrap().push((child_id, expected_costs_to_goals[child_id]));
    }

    // choose the best for each belief state
    let mut best_children: Vec<usize> = Vec::new();

    for belief_id in belief_to_children.keys() {
        let mut best_id = belief_to_children[belief_id][0].0;
        let p = transition_probability(&graph.nodes[belief_node_id].belief_state, &graph.nodes[best_id].belief_state);

        assert!(p > 0.0);
        
        let mut best_cost = p * belief_to_children[belief_id][0].1;
        for (child_id, cost) in belief_to_children[belief_id].iter().skip(0) {
            if p * *cost < best_cost {
                best_cost = p * *cost;
                best_id = *child_id;
            }
        }

        assert!(p * expected_costs_to_goals[best_id] <= expected_costs_to_goals[belief_node_id]);

        best_children.push(best_id);
    }

    best_children
}    

    

#[cfg(test)]
mod tests {

use super::*;

fn create_graph_1(belief_states: &Vec<Vec<f64>>) -> BeliefGraph<2> {
    /*
     G
    / \
  (E) (F)
   |   |
   C   D
    \ /
     B
     |
     A

     E and F are conditionally valid (2 different worlds, need to go to A to observe, when starting from B)
    */
    
    /*    
     3
    / \
  ( ) ( )
   |   |
   1   2
    \ /
     0
     |
    (4)

    bs: [p, 1.0 - p]
    */

    /*    
     10
    / \
   9  ( )
   |   |
   7   8
    \ /
     6
     |
     5

    bs: [1.0, 0.0]
    */

    /*    
    16
    / \
  ( )  15
   |   |
   13  14
    \ /
     12
     |
     11

    bs: [0.0, 1.0]
    */
    let mut belief_graph = BeliefGraph{nodes: Vec::new(), reachable_belief_states: Vec::new()};
    
    // nodes
    belief_graph.add_node([0.0, 1.0], belief_states[0].clone(), 0, BeliefNodeType::Action); // 0
    belief_graph.add_node([-1.0, 2.0], belief_states[0].clone(), 0, BeliefNodeType::Action); // 1
    belief_graph.add_node([1.0, 2.0], belief_states[0].clone(), 0, BeliefNodeType::Action); // 2
    belief_graph.add_node([0.0, 4.0], belief_states[0].clone(), 0, BeliefNodeType::Action); // 3
    belief_graph.add_node([0.0, 0.0], belief_states[0].clone(), 0, BeliefNodeType::Observation); // 4


    belief_graph.add_node([0.0, 0.0], belief_states[1].clone(), 1, BeliefNodeType::Action); // 5
    belief_graph.add_node([0.0, 1.0], belief_states[1].clone(), 1, BeliefNodeType::Action); // 6
    belief_graph.add_node([-1.0, 2.0], belief_states[1].clone(), 1, BeliefNodeType::Action);// 7
    belief_graph.add_node([1.0, 2.0], belief_states[1].clone(), 1, BeliefNodeType::Action);// 8
    belief_graph.add_node([-1.0, 3.0], belief_states[1].clone(), 1, BeliefNodeType::Action);// 9
    belief_graph.add_node([0.0, 4.0], belief_states[1].clone(), 1, BeliefNodeType::Action);// 10


    belief_graph.add_node([0.0, 0.0], belief_states[2].clone(), 2, BeliefNodeType::Action); // 11
    belief_graph.add_node([0.0, 1.0], belief_states[2].clone(), 2, BeliefNodeType::Action); // 12
    belief_graph.add_node([-1.0, 2.0], belief_states[2].clone(), 2, BeliefNodeType::Action); // 13
    belief_graph.add_node([1.0, 2.0], belief_states[2].clone(), 2, BeliefNodeType::Action); // 14
    belief_graph.add_node([10.0, 3.0], belief_states[2].clone(), 2, BeliefNodeType::Action); // 15
    belief_graph.add_node([0.0, 4.0], belief_states[2].clone(), 2, BeliefNodeType::Action); // 16


    // edges
    belief_graph.add_edge(0, 1); belief_graph.add_edge(1, 0);
    belief_graph.add_edge(0, 2); belief_graph.add_edge(2, 0);
    belief_graph.add_edge(0, 4);

    belief_graph.add_edge(4, 5); // important, belief transition
    belief_graph.add_edge(5, 6); belief_graph.add_edge(6, 5);
    belief_graph.add_edge(6, 7); belief_graph.add_edge(7, 6);
    belief_graph.add_edge(6, 8); belief_graph.add_edge(8, 6);
    belief_graph.add_edge(7, 9); belief_graph.add_edge(9, 7);
    belief_graph.add_edge(9, 10); belief_graph.add_edge(10, 9);

    belief_graph.add_edge(4, 11); // important, belief transition
    belief_graph.add_edge(11, 12); belief_graph.add_edge(12, 11);
    belief_graph.add_edge(12, 13); belief_graph.add_edge(13, 12);
    belief_graph.add_edge(12, 14); belief_graph.add_edge(14, 12);
    belief_graph.add_edge(14, 15); belief_graph.add_edge(15, 14);
    belief_graph.add_edge(15, 16); belief_graph.add_edge(16, 15);

    belief_graph
}

fn create_graph_2(belief_states: &Vec<Vec<f64>>) -> BeliefGraph<2> {
    /*
     K--J--I
     |     |
    (C)    H
     |     |
     B     G
     |     |
     A--E--F
    

     C is conditionally valid (2 different worlds, one were C is valid, the other one where B is not valid, observation in B)
    */
    
    /*    
     8--7--6
     |     |
    (?)    5
     |     |
     1     4
     |     |
     0--2--3

    bs: [p, 1.0 - p]
    */

    /*    
    17--16--15
     |      |
    (x)     14
     |      |
     10     13
     |      |
     9--11--12

    bs: [1.0, 0.0]
    */

    /*    
     27--26--25
     |       |
     20      24
     |       |
     19      23
     |       |
     18--21--22

    bs: [0.0, 1.0]
    */
    let mut belief_graph = BeliefGraph{nodes: Vec::new(), reachable_belief_states: Vec::new()};
    
    // nodes
    belief_graph.add_node([0.0, 0.0], belief_states[0].clone(), 0, BeliefNodeType::Action); // 0
    belief_graph.add_node([0.0, 1.0], belief_states[0].clone(), 0, BeliefNodeType::Observation); // 1
    belief_graph.add_node([1.0, 0.0], belief_states[0].clone(), 0, BeliefNodeType::Action); // 2
    belief_graph.add_node([2.0, 0.0], belief_states[0].clone(), 0, BeliefNodeType::Action); // 3
    belief_graph.add_node([2.0, 1.0], belief_states[0].clone(), 0, BeliefNodeType::Action); // 4
    belief_graph.add_node([2.0, 2.0], belief_states[0].clone(), 0, BeliefNodeType::Action); // 5
    belief_graph.add_node([2.0, 3.0], belief_states[0].clone(), 0, BeliefNodeType::Action); // 6
    belief_graph.add_node([1.0, 3.0], belief_states[0].clone(), 0, BeliefNodeType::Action); // 7
    belief_graph.add_node([0.0, 3.0], belief_states[0].clone(), 0, BeliefNodeType::Action); // 8

    belief_graph.add_node([0.0, 0.0], belief_states[1].clone(), 1, BeliefNodeType::Action); // 9
    belief_graph.add_node([0.0, 1.0], belief_states[1].clone(), 1, BeliefNodeType::Action); // 10
    belief_graph.add_node([1.0, 0.0], belief_states[1].clone(), 1, BeliefNodeType::Action); // 11
    belief_graph.add_node([2.0, 0.0], belief_states[1].clone(), 1, BeliefNodeType::Action); // 12
    belief_graph.add_node([2.0, 1.0], belief_states[1].clone(), 1, BeliefNodeType::Action); // 13
    belief_graph.add_node([2.0, 2.0], belief_states[1].clone(), 1, BeliefNodeType::Action); // 14
    belief_graph.add_node([2.0, 3.0], belief_states[1].clone(), 1, BeliefNodeType::Action); // 15
    belief_graph.add_node([1.0, 3.0], belief_states[1].clone(), 1, BeliefNodeType::Action); // 16
    belief_graph.add_node([0.0, 3.0], belief_states[1].clone(), 1, BeliefNodeType::Action); // 17

    belief_graph.add_node([0.0, 0.0], belief_states[1].clone(), 2, BeliefNodeType::Action); // 18
    belief_graph.add_node([0.0, 1.0], belief_states[1].clone(), 2, BeliefNodeType::Action); // 19
    belief_graph.add_node([0.0, 2.0], belief_states[1].clone(), 2, BeliefNodeType::Action); // 20
    belief_graph.add_node([1.0, 0.0], belief_states[1].clone(), 2, BeliefNodeType::Action); // 21
    belief_graph.add_node([2.0, 0.0], belief_states[1].clone(), 2, BeliefNodeType::Action); // 22
    belief_graph.add_node([2.0, 1.0], belief_states[1].clone(), 2, BeliefNodeType::Action); // 23
    belief_graph.add_node([2.0, 2.0], belief_states[1].clone(), 2, BeliefNodeType::Action); // 24
    belief_graph.add_node([2.0, 3.0], belief_states[1].clone(), 2, BeliefNodeType::Action); // 25
    belief_graph.add_node([1.0, 3.0], belief_states[1].clone(), 2, BeliefNodeType::Action); // 26
    belief_graph.add_node([0.0, 3.0], belief_states[1].clone(), 2, BeliefNodeType::Action); // 27


    // edges
    belief_graph.add_edge(0, 1);
    belief_graph.add_edge(0, 2); belief_graph.add_edge(2, 0);
    belief_graph.add_edge(2, 3); belief_graph.add_edge(3, 2);
    belief_graph.add_edge(3, 4); belief_graph.add_edge(4, 3);
    belief_graph.add_edge(4, 5); belief_graph.add_edge(5, 4);
    belief_graph.add_edge(5, 6); belief_graph.add_edge(6, 5);
    belief_graph.add_edge(6, 7); belief_graph.add_edge(7, 6);
    belief_graph.add_edge(7, 8); belief_graph.add_edge(8, 7);

    belief_graph.add_edge(1, 10); // important, belief transition
    belief_graph.add_edge(10, 9); belief_graph.add_edge(9, 10);
    belief_graph.add_edge(9, 11); belief_graph.add_edge(11, 9);
    belief_graph.add_edge(11, 12); belief_graph.add_edge(12, 11);
    belief_graph.add_edge(12, 13); belief_graph.add_edge(13, 12);
    belief_graph.add_edge(13, 14); belief_graph.add_edge(14, 13);
    belief_graph.add_edge(14, 15); belief_graph.add_edge(15, 14);
    belief_graph.add_edge(15, 16); belief_graph.add_edge(16, 15);
    belief_graph.add_edge(16, 17); belief_graph.add_edge(17, 16);

    belief_graph.add_edge(1, 19); // important, belief transition
    belief_graph.add_edge(19, 20); belief_graph.add_edge(20, 19);
    belief_graph.add_edge(20, 27); belief_graph.add_edge(27, 20);
    belief_graph.add_edge(19, 18); belief_graph.add_edge(18, 19);
    belief_graph.add_edge(18, 21); belief_graph.add_edge(21, 18);
    belief_graph.add_edge(21, 22); belief_graph.add_edge(22, 21);
    belief_graph.add_edge(22, 23); belief_graph.add_edge(23, 22);
    belief_graph.add_edge(23, 24); belief_graph.add_edge(24, 23);
    belief_graph.add_edge(24, 25); belief_graph.add_edge(25, 24);
    belief_graph.add_edge(26, 25); belief_graph.add_edge(25, 26);
    belief_graph.add_edge(27, 26); belief_graph.add_edge(26, 27);


    belief_graph
}

#[test]
fn test_conditional_dijkstra_and_extract_policy_on_graph_1() {
    let belief_states = vec![vec![0.4, 0.6], vec![1.0, 0.0], vec![0.0, 1.0]];

    let graph = create_graph_1(&belief_states);
    
    let dists = conditional_dijkstra(&graph, &vec![3, 10, 16], |a: &[f64; 2], b: &[f64; 2]| norm2(a, b) );
    let policy = extract_policy(&graph, &dists);

    // distance decrease when going towards the goal
    assert!(dists[0] < dists[1]);
    assert!(dists[0] < dists[2]);
    assert!(dists[4] < dists[0]);

    assert!(dists[6] < dists[5]);
    assert!(dists[6] < dists[8]);
    assert!(dists[7] < dists[6]);
    assert!(dists[9] < dists[7]);
    assert!(dists[10] < dists[9]);

    assert!(dists[12] < dists[11]);
    assert!(dists[12] < dists[13]);
    assert!(dists[14] < dists[12]);
    assert!(dists[15] < dists[14]);
    assert!(dists[16] < dists[15]);

    // belief transition
    assert_eq!(dists[4], belief_states[0][0] * dists[5] + belief_states[0][1] * dists[11]);

    // policy
    assert_eq!(policy.leafs.len(), 2);

    assert_eq!(policy.leaf(0).state, [0.0, 4.0]); // policy arrives to goal
    assert_eq!(policy.leaf(1).state, [0.0, 4.0]);

    assert_eq!(policy.leaf(0).belief_state, [0.0, 1.0]); // second belief first
    assert_eq!(policy.leaf(1).belief_state, [1.0, 0.0]); // first belief second

    let path_0 = policy.path_to_leaf(0);
    let path_1 = policy.path_to_leaf(1);

    assert_eq!(path_0, vec![[0.0, 1.0], [0.0, 0.0], [0.0, 0.0], [0.0, 1.0], [1.0, 2.0], [10.0, 3.0], [0.0, 4.0]]); // on the right
    assert_eq!(path_1, vec![[0.0, 1.0], [0.0, 0.0], [0.0, 0.0], [0.0, 1.0], [-1.0, 2.0], [-1.0, 3.0], [0.0, 4.0]]); // on the left
}

#[test]
fn test_conditional_dijkstra_and_extract_policy_on_graph_2() {
    let belief_states = vec![vec![0.4, 0.6], vec![1.0, 0.0], vec![0.0, 1.0]];

    let graph = create_graph_2(&belief_states);
    
    let dists = conditional_dijkstra(&graph, &vec![8, 17, 27], |a: &[f64; 2], b: &[f64; 2]| norm2(a, b) );
    let policy = extract_policy(&graph, &dists);

    // dists
    let (max_index, max_dist) = dists.iter().enumerate()
        .fold((0, 0.0), |(max_id, max), (id, val)| if *val > max{ (id, *val) } else{ (max_id, max) });
    assert_eq!(max_index, 10);
    assert_eq!(max_dist, 8.0);

    // policy
    assert_eq!(policy.leafs.len(), 2);

    assert_eq!(policy.leaf(0).state, [0.0, 3.0]); // policy arrives to goal
    assert_eq!(policy.leaf(1).state, [0.0, 3.0]);
}


#[test]
fn test_transitions() {
    assert_eq!(transition_probability(&vec![1.0, 0.0], &vec![1.0, 0.0]), 1.0);
    assert_eq!(transition_probability(&vec![0.0, 1.0], &vec![1.0, 0.0]), 0.0);

    assert_eq!(transition_probability(&vec![0.4, 0.6], &vec![0.4, 0.6]), 1.0);
    assert_eq!(transition_probability(&vec![0.4, 0.6], &vec![1.0, 0.0]), 0.4);
    assert_eq!(transition_probability(&vec![0.5, 0.0, 0.5, 0.0], &vec![0.0, 0.5, 0.0, 0.5]), 0.0);
}
}