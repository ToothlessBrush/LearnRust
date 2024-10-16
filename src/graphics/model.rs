extern crate gltf;
use glm::{Mat4, Vec3, Vec4};
use gltf::{image::Source, scene::Transform};
use std::{collections::HashMap, path::Path, primitive, rc::Rc};

use crate::utils::fps_manager::{self, FPSManager};
use crate::utils::input_manager::InputManager;

use super::{
    buffers::{self, vertex_buffer::Vertex},
    camera::Camera3D,
    mesh::Mesh,
    shader::Shader,
    texture::Texture,
};

struct NodeTransform {
    translation: Vec3,
    rotation: glm::Quat,
    scale: Vec3,
}

// struct MeshPrimitive {
//     vertices: Vec<Vertex>,
//     indices: Vec<u32>,
//     textures: Vec<Texture>,
// }

struct Node {
    name: String,
    transform: NodeTransform,
    transform_matrix: Mat4,
    mesh_primitives: Vec<Mesh>,
}

pub struct Model {
    nodes: Vec<Node>,
    ready_callback: Option<fn()>,
    behavior_callback: Option<Box<dyn fn(&FPSManager, &InputManager)>>,
}

impl Model {
    pub fn new(file: &str) -> Model {
        let gltf = gltf::import(Path::new(file)).expect("failed to open GLTF file");
        let (doc, buffers, images) = gltf;

        let mut nodes: Vec<Node> = Vec::new();

        let mut texture_cache: HashMap<usize, Rc<Texture>> = HashMap::new(); //cache with key as image index and value as a smart pointer to the texture

        for node in doc.nodes() {
            println!("loading Node: {:?}", node.name().unwrap());
            //get node transformation data
            let mut matrix = Mat4::identity();
            let (translation, rotation, scale) = node.transform().decomposed();
            let translation: Vec3 = glm::make_vec3(&translation);
            let rotation: Vec4 = glm::make_vec4(&rotation);
            let scale: Vec3 = glm::make_vec3(&scale);

            let quat_rotation = glm::quat(rotation.x, rotation.y, rotation.z, rotation.w);

            let translation_matrix = glm::translate(&Mat4::identity(), &translation);
            let rotation_matrix = glm::quat_to_mat4(&quat_rotation);
            let scale_matrix = glm::scale(&Mat4::identity(), &scale);

            //get matrix from translation, rotation, and scale
            matrix = translation_matrix * rotation_matrix * scale_matrix;

            if let Some(mesh) = node.mesh() {
                let mut primitive_meshes: Vec<Mesh> = Vec::new();
                for primitive in mesh.primitives() {
                    let reader = primitive.reader(|buffer| Some(&buffers[buffer.index()]));

                    //get vertex data from reader
                    let positions: Vec<[f32; 3]> = reader.read_positions().unwrap().collect();
                    let normals: Vec<[f32; 3]> = reader.read_normals().unwrap().collect();
                    let tex_coords: Vec<[f32; 2]> =
                        reader.read_tex_coords(0).unwrap().into_f32().collect();
                    //read color data if it exists otherwise set color to white
                    let color = if let Some(colors) = reader.read_colors(0) {
                        let colors: Vec<[f32; 4]> = colors.into_rgba_f32().collect();
                        glm::make_vec4(&colors[0])
                    } else {
                        glm::vec4(1.0, 1.0, 1.0, 1.0)
                    };

                    let indices = if let Some(indices) = reader.read_indices() {
                        indices.into_u32().collect::<Vec<u32>>()
                    } else {
                        Vec::new()
                    };

                    //construct vertices from the extracted data
                    let vertices: Vec<Vertex> = positions
                        .into_iter()
                        .enumerate()
                        .map(|(i, pos)| Vertex {
                            position: glm::make_vec3(&pos),
                            normal: glm::make_vec3(&normals[i]),
                            texUV: glm::make_vec2(&tex_coords[i]),
                            color,
                        })
                        .collect();

                    //load textures
                    let mut textures: Vec<Rc<Texture>> = Vec::new();

                    let alpha_mode = primitive.material().alpha_mode();

                    let double_sided = primitive.material().double_sided();

                    let base_color: glm::Vec4 = glm::make_vec4(
                        &primitive
                            .material()
                            .pbr_metallic_roughness()
                            .base_color_factor(),
                    );

                    //load diffuse texture
                    if let Some(material) = primitive
                        .material()
                        .pbr_metallic_roughness()
                        .base_color_texture()
                    {
                        let image_index = material.texture().source().index();
                        let shared_texture = texture_cache //check if the texture is already loaded if so then use the cached texture to avoid loading the same texture multiple times
                            .entry(image_index)
                            .or_insert_with(|| {
                                let image = &images[image_index];
                                let format = if image.format == gltf::image::Format::R8G8B8A8 {
                                    gl::RGBA
                                } else if image.format == gltf::image::Format::R8G8B8 {
                                    gl::RGB
                                } else if image.format == gltf::image::Format::R8 {
                                    gl::RED
                                } else {
                                    panic!("unsupported image format not rgba, rgb, or r");
                                };
                                Rc::new(Texture::load_from_gltf(
                                    &image.pixels,
                                    image.width,
                                    image.height,
                                    "diffuse",
                                    format,
                                ))
                            })
                            .clone();

                        textures.push(shared_texture);
                    };

                    //load specular texture (we load the metallic roughness texture as the specular texture since metallic roughtness is the closest thing to specular in gltf)
                    if let Some(material) = primitive
                        .material()
                        .pbr_metallic_roughness()
                        .metallic_roughness_texture()
                    {
                        let image_index = material.texture().source().index();
                        let shared_texture = texture_cache
                            .entry(image_index)
                            .or_insert_with(|| {
                                let image = &images[image_index];
                                let format = if image.format == gltf::image::Format::R8G8B8A8 {
                                    //rgba format
                                    gl::RGBA
                                } else if image.format == gltf::image::Format::R8G8B8 {
                                    //rgb format
                                    gl::RGB
                                } else {
                                    gl::RGB
                                };
                                Rc::new(Texture::load_from_gltf(
                                    &image.pixels,
                                    image.width,
                                    image.height,
                                    "specular",
                                    format,
                                ))
                            })
                            .clone();

                        textures.push(shared_texture);
                    }

                    //create the mesh
                    let mesh = Mesh::new(vertices, indices, textures, base_color, double_sided);
                    primitive_meshes.push(mesh);
                }

                let node = Node {
                    name: node.name().unwrap_or_default().to_string(),
                    transform: NodeTransform {
                        translation,
                        rotation: quat_rotation,
                        scale,
                    },
                    transform_matrix: matrix,
                    mesh_primitives: primitive_meshes,
                };
                nodes.push(node);
            }
        }

        println!("successfully loaded model: {}", file);
        Model { nodes: nodes }
    }

    pub fn draw(&mut self, shader: &mut Shader, camera: &Camera3D) {
        // self.nodes.sort_by(|a, b| {
        //     let dist_a = glm::distance(&a.transform.translation, &camera.get_position());
        //     let dist_b = glm::distance(&b.transform.translation, &camera.get_position());
        //     dist_b.partial_cmp(&dist_a).unwrap()
        // });

        for node in &self.nodes {
            shader.bind();
            shader.set_uniform_mat4f("u_Model", &node.transform_matrix);
            //println!("drawing node: {}", node.transform_matrix);

            for mesh in &node.mesh_primitives {
                mesh.draw(shader, camera);
            }
        }
    }

    pub fn translate(&mut self, translation: Vec3) {
        for node in &mut self.nodes {
            node.transform.translation += translation;
            node.transform_matrix = glm::translate(&node.transform_matrix, &translation);
        }
    }

    pub fn rotate(&mut self, axis: Vec3, degrees: f32) {
        let radians = glm::radians(&glm::vec1(degrees)).x;

        let rotation_quat = glm::quat_angle_axis(radians, &axis);

        for node in &mut self.nodes {
            node.transform.rotation = rotation_quat * node.transform.rotation;
            node.transform_matrix = glm::quat_to_mat4(&rotation_quat) * node.transform_matrix;
        }
    }

    pub fn scale(&mut self, scale: Vec3) {
        for node in &mut self.nodes {
            node.transform.scale += scale;
            node.transform_matrix = glm::scale(&node.transform_matrix, &scale);
        }
    }

    pub fn define_ready(&mut self, ready_function: fn()) {
        self.ready_callback = Some(ready_function);
    }

    pub fn define_behavior<F>(&mut self, behavior_function: F) -> &mut Model 
    where 
        F: Fn(&FPSManager, &InputManager) + 'static, 
    {
        self.behavior_callback = Some(Box::new(behavior_function));
        self
    }

    //if the model has a ready function then call it
    pub fn ready(&self) {
        if let Some(callback) = self.ready_callback {
            callback();
        }
    }

    //if the model has a behavior function then call it
    pub fn behavior(&self, fps_manager: &FPSManager, input_manager: &InputManager) {
        if let Some(callback) = self.behavior_callback {
            callback(fps_manager, input_manager);
        }
    }
}
