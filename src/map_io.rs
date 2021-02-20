use crate::{rrt::{Reachable, RRTFuncs, RRTTree}};
use crate::{prm_graph::{PRMGraph, PRMNode, PRMFuncs}};
use crate::common::*;
use image::{DynamicImage, GenericImageView, Luma};
use image::DynamicImage::ImageLuma8;
use core::f64;
use std::vec::Vec;
use std::collections::HashSet;
extern crate queues;
use queues::*;
use bitvec::prelude::*;

#[derive(Debug, PartialEq)]
pub enum Belief {
	Free,
	Obstacle,
	Zone(usize),
}

#[derive(Clone)]
pub struct Map
{
	img: image::GrayImage,
	low: [f64; 2],
	//up: [f64; 2], // low + up are enough and make up redundant
	ppm: f64,
	zones: Option<image::GrayImage>,
	n_zones: usize,
	n_worlds: usize,
	zones_to_worlds: Vec<WorldMask>
}

// Given N zones, there are 2^N possible worlds
impl Map {
	pub fn open(filepath : &str, low: [f64; 2], up: [f64; 2]) -> Self {
		Self::build(Self::open_image(filepath), low, up)
	}

	pub fn save(&self, filepath: &str) {
		self.img.save(filepath).expect("Couldn't save image");
	}

	fn build(img: image::GrayImage, low: [f64; 2], up: [f64; 2])-> Map {
		let ppm = (img.width() as f64) / (up[0] - low[0]);

		Map{img, low, /*up,*/ ppm, zones: None, n_zones: 0, n_worlds: 0, zones_to_worlds: Vec::new()}
	}


	fn open_image(filepath : &str) -> image::GrayImage {
		let img = image::open(filepath).expect(&format!("Impossible to open image: {}", filepath));

		match img {
			ImageLuma8(gray_img) => gray_img,
			_ => panic!("Wrong image format!"),
		}
	}

	// zones specific
	pub fn add_zones(&mut self, filepath : &str) {
		// image
		self.zones = Some(Self::open_image(filepath));

		// number of zones
		let mut max_id = 0;
		for i in 0..self.zones.as_ref().unwrap().height() {
			for j in 0..self.zones.as_ref().unwrap().width() {
				let z = self.get_zone_index(i, j);
				match z {
					Some(id) => {
						if id > max_id {
							max_id = id;
						}
					},
					None => {}
				}	
			}
		}
		
		self.n_zones = max_id + 1;
		self.n_worlds = (2 as u32).pow(self.n_zones as u32) as usize;

		// zone -> worlds
		for i in 0..self.n_zones {
			self.zones_to_worlds.push(self.zone_index_to_world_mask(i));
		}
	}

	pub fn is_state_valid(&self, xy: &[f64; 2]) -> bool {
		let ij = self.to_pixel_coordinates(&*xy);
		let p = self.img.get_pixel(ij[1], ij[0]);

		p[0] > 0
	}

	pub fn is_state_valid_2(&self, xy: &[f64; 2]) -> Belief {
		let ij = self.to_pixel_coordinates(&*xy);
		let p = self.img.get_pixel(ij[1], ij[0]);

		match p[0] {
			255 => Belief::Free,
			0 => Belief::Obstacle,
			_ => Belief::Zone(self.get_zone_index(ij[0], ij[1]).unwrap())
		}
	}

	fn to_pixel_coordinates(&self, xy: &[f64; 2]) -> [u32; 2] {
		let i: u32 = (((self.img.height() - 1) as f64) - (xy[1] - self.low[1]) * self.ppm) as u32;
		let j: u32 = ((xy[0] - self.low[0]) * self.ppm) as u32;

		[i, j]
	}

	fn get_zone_index(&self, i: u32, j: u32) -> Option<usize> {
		let p = self.zones.as_ref().expect("Zones missing").get_pixel(j, i);
		match p[0] {
			255 => None,
			p => Some(p as usize),
		}
	}

	fn zone_index_to_world_mask(&self, zone_index: usize) -> WorldMask {
		let mut world_mask = bitvec![1; self.n_worlds];
		for world in 0..self.n_worlds {
			if !self.get_zone_status(world, zone_index).expect("Call with a correct world id") {
				world_mask.set(world, false);
			}
		}
		world_mask
	}

	fn get_zone_status(&self, world: usize, zone_index: usize) -> Result<bool, ()> {
		if zone_index < self.n_zones && world < self.n_worlds {
			Ok(world & (1 << zone_index) != 0)
		} else {
			Err(())
		}
	}

	fn get_traversed_space(&self, a: &[f64; 2], b: &[f64; 2]) -> Belief {
		let mut traversed_space = Belief::Free;

		let a_ij = self.to_pixel_coordinates(a);
		let b_ij = self.to_pixel_coordinates(b);

		let a = (a_ij[0] as i32, a_ij[1] as i32);
		let b = (b_ij[0] as i32, b_ij[1] as i32);

		for (i, j) in line_drawing::Bresenham::new(a, b) {
			let pixel = self.img.get_pixel(j as u32, i as u32);
			match pixel[0] {
				255 => {},
				0 => {return Belief::Obstacle; },
				_ => {					
					traversed_space = Belief::Zone(self.get_zone_index(i as u32, j as u32).unwrap());
				}
			};
		}

		traversed_space
	}

	// drawing functions
	pub fn resize(&mut self, factor: u32) {
		let w = self.img.width() * factor;
		let h = self.img.height() * factor;
		
		let resized_im = DynamicImage::ImageLuma8(self.img.clone()).resize(w, h, image::imageops::FilterType::Lanczos3);
		self.img = match resized_im {
			ImageLuma8(gray_img) => gray_img,
			_ => panic!("Wrong image format!"),
		};

		self.ppm *= factor as f64;

		if let Some(zone_img) = &self.zones {
			let resized_zones = DynamicImage::ImageLuma8(zone_img.clone()).resize(w, h, image::imageops::FilterType::Lanczos3);
			self.zones = match resized_zones {
			ImageLuma8(gray_img) => Some(gray_img),
			_ => panic!("Wrong image format!"),
		};
		}
	}

	pub fn draw_path(&mut self, path: Vec<[f64;2]>) {
		for i in 1..path.len() {
			self.draw_line(path[i-1], path[i], 0);
		}
	}

	pub fn draw_tree(&mut self, rrttree: &RRTTree<2>) {
		for c in &rrttree.nodes {
			for parent in &c.parents {
				let parent = &rrttree.nodes[parent.id];
				self.draw_line(parent.state, c.state, 180);
			}
		}
	}

	fn draw_line(&mut self, a: [f64; 2], b: [f64; 2], color: u8) {
		let a_ij = self.to_pixel_coordinates(&a);
		let b_ij = self.to_pixel_coordinates(&b);

		let a = (a_ij[0] as f32, a_ij[1] as f32);
		let b = (b_ij[0] as f32, b_ij[1] as f32);

		for ((i, j), value) in line_drawing::XiaolinWu::<f32, i32>::new(a, b) {
			if 0 <= i && i < self.img.height() as i32 && 0 <= j && j < self.img.width() as i32 {
				let pixel = self.img.get_pixel_mut(j as u32, i as u32);
				let old_color = pixel.0[0];
				let new_color = ((1.0 - value) * (old_color as f32) + value * (color as f32)) as u8;
				pixel.0 = [new_color];
			}
		}
	}

	pub fn draw_full_graph(&mut self, graph: &PRMGraph<2>) {
		for from in &graph.nodes {
			for to_id in from.children.clone() {
				let to  = &graph.nodes[to_id];
				self.draw_line(from.state, to.state, 200);
			}
		}
	}

	pub fn draw_graph_for_world(&mut self, graph: &PRMGraph<2>, world:usize) {
		if world > self.n_worlds {
			panic!("Invalid world id");
		}

		let mut visited = HashSet::new();
		let mut queue: Queue<usize> = queue![];
		visited.insert(0);
		queue.add(0).expect("Overflow!");

		while queue.size() > 0 {
			let from_id = queue.remove().unwrap();
			let from = &graph.nodes[from_id];

			for &to_id in &from.children {
				let to = &graph.nodes[to_id];

				if to.validity[world] {
					self.draw_line(from.state, to.state, 100);

					if !visited.contains(&to_id) {
						queue.add(to_id).expect("Overflow");
						visited.insert(to_id);
					}
				}
			}
		}
	}

	pub fn draw_world(&mut self, world_id:usize) {
		for i in 0..self.zones.as_ref().unwrap().height() {
			for j in 0..self.zones.as_ref().unwrap().width() {
				let z = self.get_zone_index(i, j);

				match z {
					Some(zone_id) => {
							let color = if self.zones_to_worlds[zone_id][world_id] { 255 } else { 0 };
							self.img.put_pixel(j, i, Luma([color]));
					},
					None => {}
				}
			}
		}
	}
} 

impl RRTFuncs<2> for Map {
	fn state_validator(&self, state: &[f64; 2]) -> bool {
		self.is_state_valid(state)
	}

	fn transition_validator(&self, a: &[f64; 2], b: &[f64; 2]) -> Reachable {
		let space = self.get_traversed_space(a, b);

		match space {
			Belief::Free => { Reachable::Always },
			Belief::Obstacle => { Reachable::Never },
			Belief::Zone(zone) => { Reachable::Restricted(&self.zones_to_worlds[zone]) }
		}
	}
}

impl PRMFuncs<2> for Map {
	fn state_validity(&self, state: &[f64; 2]) -> Option<WorldMask> {
		match self.is_state_valid_2(state) {
			Belief::Zone(zone_index) => {Some(self.zones_to_worlds[zone_index].clone())}, // TODO: improve readability
			Belief::Free => {Some(bitvec![1; self.n_worlds])},
			Belief::Obstacle => None
		}
	}

	fn transition_validator(&self, from: &PRMNode<2>, to: &PRMNode<2>) -> bool {
		let symbolic_validity = from.validity.iter().zip(&to.validity)
		.any(|(a, b)| *a && *b);

		let geometric_validitiy = self.get_traversed_space(&from.state, &to.state) != Belief::Obstacle;

		symbolic_validity && geometric_validitiy
	}
}

#[cfg(test)]
mod tests {

use super::*;
use std::fs;
use std::path::Path;

#[test]
fn open_image() {
	Map::open("data/map0.pgm", [-1.0, -1.0], [1.0, 1.0]);
}

#[test]
fn test_valid_state() {
	let m = Map::open("data/map0.pgm", [-1.0, -1.0], [1.0, 1.0]);
	assert!(m.is_state_valid(&[0.0, 0.0]));
}

#[test]
fn test_invalid_state() {
	let m = Map::open("data/map0.pgm", [-1.0, -1.0], [1.0, 1.0]);
	assert!(!m.is_state_valid(&[0.0, 0.6]));
}

#[test]
fn clone_draw_line_and_save_image() {
	let m = Map::open("data/map0.pgm", [-1.0, -1.0], [1.0, 1.0]);
	let mut m = m.clone();

	m.draw_line([-0.5, -0.5], [0.5, 0.5], 128);
	m.draw_path([[-0.3, -0.4], [0.0, 0.0] ,[0.4, 0.3]].to_vec());

	m.save("results/tmp.pgm");

	assert!(Path::new("results/tmp.pgm").exists());
	fs::remove_file("results/tmp.pgm").unwrap();
}
// MAP 1 zone
#[test]
fn test_map_1_construction() {
	let mut map = Map::open("data/map1.pgm", [-1.0, -1.0], [1.0, 1.0]);
	map.add_zones("data/map1_zone_ids.pgm");

	assert_eq!(map.n_zones, 1);
	assert_eq!(map.n_worlds, 2);
}

#[test]
fn test_map_1_states() {
	let mut map = Map::open("data/map1.pgm", [-1.0, -1.0], [1.0, 1.0]);
	map.add_zones("data/map1_zone_ids.pgm");

	// zone status
	assert_eq!(map.get_zone_status(0, 0).unwrap(), false);// world 0 corresponds to all variations invalid
	assert_eq!(map.get_zone_status(1, 0).unwrap(), true); // world 1 corresponds to door with index 2^0 open

	assert_eq!(map.get_zone_status(1, 1), Err(())); // zone id too high
	assert_eq!(map.get_zone_status(2, 0), Err(())); // world number too high

	// world validities
	assert_eq!(map.state_validity(&[0.1, 0.12]), None); // obstacle
	assert_eq!(map.state_validity(&[0.0, 0.0]).unwrap(), bitvec![1, 1]); // free space
	assert_eq!(map.state_validity(&[0.57, 0.11]).unwrap(), bitvec![0, 1]); // on door
}

// MAP 2 zones
#[test]
fn test_map_2_construction() {
	let mut map = Map::open("data/map2.pgm", [-1.0, -1.0], [1.0, 1.0]);
	map.add_zones("data/map2_zone_ids.pgm");

	assert_eq!(map.n_zones,2);
	assert_eq!(map.n_worlds, 4);
}

#[test]
fn test_map_2_states() {
	let mut map = Map::open("data/map2.pgm", [-1.0, -1.0], [1.0, 1.0]);
	map.add_zones("data/map2_zone_ids.pgm");

	// world validity
	assert_eq!(map.state_validity(&[0.1, 0.12]), None); // obstacle
	assert_eq!(map.state_validity(&[0.0, 0.0]).unwrap(), bitvec![1,1,1,1]); // free space

	assert_eq!(map.state_validity(&[0.51, -0.41]).unwrap(), bitvec![0,1,0,1]); // zone 0
	assert_eq!(map.state_validity(&[0.57, 0.09]).unwrap(), bitvec![0,0,1,1]); // zone 1
}

// MAP 4 zones
#[test]
fn test_map_4_construction() {
	let mut map = Map::open("data/map4.pgm", [-1.0, -1.0], [1.0, 1.0]);
	map.add_zones("data/map4_zone_ids.pgm");

	assert_eq!(map.n_zones, 4);
	assert_eq!(map.n_worlds, 16);
}

#[test]
fn test_map_4_states() {
	let mut map = Map::open("data/map4.pgm", [-1.0, -1.0], [1.0, 1.0]);
	map.add_zones("data/map4_zone_ids.pgm");

	// door validity
	assert_eq!(map.is_state_valid_2(&[0.0, 0.0]), Belief::Free);
	assert_eq!(map.is_state_valid_2(&[0.0, -0.24]), Belief::Obstacle);
	assert_eq!(map.is_state_valid_2(&[-0.67, -0.26]), Belief::Zone(0));

	// world validities
	assert_eq!(map.state_validity(&[-0.29, -0.23]), None);
	assert_eq!(map.state_validity(&[0.0, 0.0]).unwrap(), bitvec![1; 16]);

	// world ennumerations
	let mut world_mask = bitvec![0; 16];
	let zone = 1;

	for i in 0..map.n_worlds {
		if i & (zone << 1) != 0 {
			*world_mask.get_mut(i).unwrap() = true;
		}
	}

	assert_eq!(map.zones_to_worlds[zone], world_mask);
}

#[test]
fn test_resize_image() {
	let mut m = Map::open("data/map0.pgm", [-1.0, -1.0], [1.0, 1.0]);
	m.resize(2);
	
	m.save("results/tmp.pgm");
	assert!(Path::new("results/tmp.pgm").exists());
	fs::remove_file("results/tmp.pgm").unwrap();
}

}
