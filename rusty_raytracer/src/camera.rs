// responsible for:
// - constructing and dispatching rays into the world
// - using the results of these rays to construct the rendered image

use std::io::Write;
use std::fs::File;
use std::io;
use std::sync::Arc;
use nalgebra::{Point3, Vector3};
use rayon::prelude::*;
use rayon::iter::IntoParallelIterator;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::colour::write_colour_string;
use crate::interval::Interval;
use crate::{degrees_to_radians, random_f32, random_in_unit_disk};
use crate::{hittable::Hittable, hittable_list::HittableList, ray::Ray, colour::{write_colour, Colour}};

pub struct Camera {
    pub aspect_ratio: f32,
    pub image_width: f32,
    pub samples_per_pixel: u32,
    pub max_depth: u32,
    pub vfov: u32,
    pub lookfrom: Point3<f32>,
    pub lookat: Point3<f32>,
    pub vup: Vector3<f32>,
    pub defocus_angle: f32,
    pub focus_dist: f32,
    image_height: f32,
    pixel_samples_scale: f32,
    center: Point3<f32>,
    pixel00_loc: Point3<f32>,
    pixel_delta_u: Vector3<f32>,
    pixel_delta_v: Vector3<f32>,
    u: Vector3<f32>,
    v: Vector3<f32>,
    w: Vector3<f32>,
    defocus_disk_u: Vector3<f32>,
    defocus_disk_v: Vector3<f32>,
}

impl Camera {
    pub fn new() -> Self {
        Self {
            aspect_ratio: 1.0,
            image_width: 100.0,
            samples_per_pixel: 10,
            max_depth: 10,
            vfov: 90,
            lookfrom: Point3::origin(),
            lookat: Point3::new(0.0, 0.0, -1.0),
            vup: Vector3::new(0.0, 1.0, 0.0),
            defocus_angle: 0.0,
            focus_dist: 10.0,
            image_height: 0.0,
            pixel_samples_scale: 0.0,
            center: Point3::origin(),
            pixel00_loc: Point3::origin(),
            pixel_delta_u: Vector3::zeros(),
            pixel_delta_v: Vector3::zeros(),
            u: Vector3::zeros(),
            v: Vector3::zeros(),
            w: Vector3::zeros(),
            defocus_disk_u: Vector3::zeros(),
            defocus_disk_v: Vector3::zeros(),
        }
    }
    pub fn initialise(&mut self) {
        // Ensure dimensions are correctly set
        self.image_height = self.image_width / self.aspect_ratio;
        if self.image_height < 1.0 {
            self.image_height = 1.0;
        }
        
        if self.samples_per_pixel == 0 {self.samples_per_pixel = 100}
        self.pixel_samples_scale = 1.0/(self.samples_per_pixel as f32);
    
        self.center = self.lookfrom; 

        // Camera setup
        let theta = degrees_to_radians(self.vfov as f32);
        let h = (theta/2.0).tan();
        let viewport_height = 2.0*h*self.focus_dist;
        let viewport_width = viewport_height * (self.image_width / self.image_height);
    
        // basis vecs for camera coord frame
        self.w = (self.lookfrom - self.lookat).normalize();
        self.u = (self.vup.cross(&self.w)).normalize();
        self.v = self.w.cross(&self.u);

        // Ensure viewport sizes are sensible
        println!("Viewport width: {}", viewport_width);
        println!("Viewport height: {}", viewport_height);
        println!("lookfrom: {}", self.lookfrom);
        println!("lookat: {}", self.lookat);
        println!("vfov: {}", self.vfov);
        println!("aspect_ratio: {}", self.aspect_ratio);
    
        // Vectors along viewport edges
        let viewport_u = viewport_width*self.u;
        let viewport_v = viewport_height*-self.v;
    
        // Pixel deltas
        self.pixel_delta_u = viewport_u / self.image_width;
        self.pixel_delta_v = viewport_v / self.image_height;
    
        // Upper-left pixel location
        let viewport_upper_left = self.center
            - (self.focus_dist*self.w)
            - viewport_u / 2.0
            - viewport_v / 2.0;
    
        self.pixel00_loc = viewport_upper_left + 0.5 * (self.pixel_delta_u + self.pixel_delta_v);
    
        let defocus_radius = self.focus_dist * (degrees_to_radians(self.defocus_angle/2.0)).tan();
        self.defocus_disk_u = self.u*defocus_radius;
        self.defocus_disk_v = self.v*defocus_radius;
        // Double-check deltas
        println!("Pixel deltas: u = {:?}, v = {:?}", self.pixel_delta_u, self.pixel_delta_v);
    }

    pub fn get_ray(&self, i: usize, j: usize) -> Ray {
        let offset = sample_square();
        let pixel_sample = self.pixel00_loc
                            + ((i as f32 + offset.x) * self.pixel_delta_u)
                            + ((j as f32 + offset.y) * self.pixel_delta_v);
        
        let ray_origin = if self.defocus_angle <= 0.0 {self.center} else {self.defocus_disk_sample()};
        let ray_direction = pixel_sample - ray_origin;

        Ray::new_from(ray_origin, ray_direction)
    }

    pub fn render(&mut self, world: &Arc<&HittableList>) -> io::Result<()> {

        self.initialise();
        // render
        let mut file = File::create("rendered_image.ppm")?;
    
        let header = format!("P3\n{} {}\n255\n", self.image_width, self.image_height);
    
        file.write_all(header.as_bytes())?;

        let progress = Arc::new(AtomicUsize::new(0)); // Shared progress counter

        let rendered_rows: Vec<String> = (0..self.image_height as usize)
            .into_par_iter() // Parallelize the outer loop
            .map(|j| {
                let mut row = String::new();
                let my_world = Arc::clone(&world);
                for i in 0..self.image_width as usize {
                    let mut pixel_colour = Colour::new();
                    for _ in 0..self.samples_per_pixel {
                        let r = self.get_ray(i, j);
                        pixel_colour.0 += ray_colour(&r, self.max_depth, &my_world).0;
                    }
                    pixel_colour.0 *= self.pixel_samples_scale;
                    row.push_str(&format!("{}\n", write_colour_string(pixel_colour)));
                }
                let completed = progress.fetch_add(1, Ordering::Relaxed) + 1;
                if completed % 1 == 0 || completed == self.image_height as usize {
                    println!("Progress: {}/{}", completed, self.image_height);
                }
                row
            })
            .collect();

        // Write the computed rows to the file sequentially
        for row in rendered_rows {
            write!(file, "{}", row).unwrap();
        }
        println!("\rDone.               \n");
        Ok(())
    }
    fn defocus_disk_sample(&self) -> Point3<f32> {
        let p = random_in_unit_disk();
        return self.center + (p[0]*self.defocus_disk_u) + (p[1]*self.defocus_disk_v);
    }
}

fn ray_colour(ray: &Ray, depth: u32, world: &Arc<&HittableList>) -> Colour {
    if depth <= 0 {return Colour::new()};

    let my_world = Arc::clone(&world);
    
    if let Some(hit_rec) = my_world.hit(ray, &Interval::new(0.001, f32::INFINITY)) {
        //set face normal
        if let Some((attenuation, scattered)) = hit_rec.mat.scatter(&ray, &hit_rec) { 
            let r_col = ray_colour(&scattered, depth-1, &my_world);
            
            return Colour::new_from(attenuation.r()*r_col.r(), attenuation.g()*r_col.g(), attenuation.b()*r_col.b())
        }
        return Colour::new()
    }

    // else draw sky
    let unit_direction = ray.direction().normalize();
    let a = 0.5 * (unit_direction.y + 1.0);
    let ans = (1.0-a)*Colour::new_from(1.0, 1.0, 1.0).0 + a*Colour::new_from(0.5, 0.7, 1.0).0;
    Colour::new_from(ans[0], ans[1], ans[2])
}

fn sample_square() -> Vector3<f32> {
    Vector3::new(random_f32() - 0.5, random_f32() - 0.5, 0.0)
}

