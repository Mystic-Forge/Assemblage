use std::{collections::HashMap, fs, sync::Arc};

use glam::IVec3;

use crate::voxels::biome_profile::instructions::{
    DensityInstruction, DepthInstruction, MoistureInstruction, TemperatureInstruction,
};

use self::instructions::{
    AddInstruction, ConstInstruction, IfInstruction, Instruction, LessInstruction,
    SimplexInstruction,
};

use super::{
    voxel_data::VoxelData,
    voxel_registry::get_voxel_by_name,
    voxel_shapes::{self, voxel_shape, VoxelShape},
};

lazy_static! {
    static ref BIOMES: HashMap<String, BiomeProfile> = load_biomes();
}

fn load_biomes() -> HashMap<String, BiomeProfile> {
    let paths = fs::read_dir("./src/resources/biome_profiles/").unwrap();
    let mut map = HashMap::new();

    for biome_file in paths.into_iter() {
        let biome_file = biome_file.unwrap();

        let name = biome_file
            .file_name()
            .to_string_lossy()
            .replace(".json", "");

        map.insert(
            name.to_string(),
            BiomeProfile::from_json(fs::read_to_string(biome_file.path()).unwrap()),
        );

        println!("==Created Biome Profile==");
        println!("Name: {name}");
        println!("");
    }

    return map;
}

pub fn get_biome_by_name(name: String) -> Option<&'static BiomeProfile> {
    BIOMES.get(&name)
}

pub struct BiomeProfile {
    density_formula: Arc<Box<dyn Instruction<f32>>>,
    id_formula: Arc<Box<dyn Instruction<u16>>>,
    shape_formula: Arc<Box<dyn Instruction<VoxelShape>>>,
}

impl BiomeProfile {
    pub fn from_json(data: String) -> Self {
        let json: serde_json::Value = serde_json::from_str(&data).unwrap();
        let mut fields: HashMap<&str, Arc<Box<dyn Instruction<f32>>>> = HashMap::new();
        for field in json.get("Samplers").unwrap().as_array().unwrap() {
            let field_type = field.get("Type").unwrap().as_str().unwrap();
            let field_name = field.get("Name").unwrap().as_str().unwrap();
            fields.insert(
                field_name,
                match field_type {
                    "Simplex" => Arc::new(Box::new(SimplexInstruction {
                        wavelength: field.get("Wavelength").unwrap().as_f64().unwrap() as f32,
                        amplitude: field.get("Amplitude").unwrap().as_f64().unwrap() as f32,
                    })),
                    "Formula" => build_f32_instruction(
                        field.get("Formula").unwrap().as_str().unwrap().to_string(),
                        &fields,
                    ),
                    &_ => panic!("Field type is not supported: {field_type}"),
                },
            );
        }
        Self {
            density_formula: build_f32_instruction(
                json.get("Voxel Density")
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string(),
                &fields,
            ),
            id_formula: build_voxel_type_instruction(
                json.get("Voxel Type")
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string(),
                &fields,
            ),
            shape_formula: build_voxel_shape_instruction(
                json.get("Voxel Shape")
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string(),
                &fields,
            ),
        }
    }

    pub fn sample_density(&self, context: &SampleContext) -> f32 {
        self.density_formula.process(context)
    }

    pub fn sample_voxel(&self, context: &SampleContext) -> VoxelData {
        let id = self.id_formula.process(context);
        let shape = self.shape_formula.process(context);
        VoxelData {
            shape,
            state: 0,
            id,
        }
    }
}

mod instructions {
    use std::sync::Arc;

    use noise::{NoiseFn, Perlin};

    use super::SampleContext;

    pub trait Instruction<T>: Sync + Send {
        fn process(&self, context: &SampleContext) -> T;
    }

    pub struct ConstInstruction<T> {
        pub val: T,
    }

    impl<T: Copy + Sync + Send> Instruction<T> for ConstInstruction<T> {
        fn process(&self, context: &SampleContext) -> T {
            self.val
        }
    }

    pub struct SubtractInstruction {
        pub val1: Arc<Box<dyn Instruction<f32>>>,
        pub val2: Arc<Box<dyn Instruction<f32>>>,
    }

    impl Instruction<f32> for SubtractInstruction {
        fn process(&self, context: &SampleContext) -> f32 {
            self.val1.process(context) - self.val2.process(context)
        }
    }

    pub struct AddInstruction {
        pub val1: Arc<Box<dyn Instruction<f32>>>,
        pub val2: Arc<Box<dyn Instruction<f32>>>,
    }

    impl Instruction<f32> for AddInstruction {
        fn process(&self, context: &SampleContext) -> f32 {
            self.val1.process(context) + self.val2.process(context)
        }
    }

    pub struct IfInstruction<T> {
        pub condition: Arc<Box<dyn Instruction<bool>>>,
        pub val1: Arc<Box<dyn Instruction<T>>>,
        pub val2: Arc<Box<dyn Instruction<T>>>,
    }

    impl<T> Instruction<T> for IfInstruction<T> {
        fn process(&self, context: &SampleContext) -> T {
            if self.condition.process(context) {
                self.val1.process(context)
            } else {
                self.val2.process(context)
            }
        }
    }

    pub struct LessInstruction {
        pub val1: Arc<Box<dyn Instruction<f32>>>,
        pub val2: Arc<Box<dyn Instruction<f32>>>,
    }

    impl Instruction<bool> for LessInstruction {
        fn process(&self, context: &SampleContext) -> bool {
            self.val1.process(context) < self.val2.process(context)
        }
    }

    #[derive(Clone)]
    pub struct SimplexInstruction {
        pub wavelength: f32,
        pub amplitude: f32,
    }

    lazy_static! {
        static ref PERLIN: Perlin = Perlin::new();
    }

    impl Instruction<f32> for SimplexInstruction {
        fn process(&self, context: &SampleContext) -> f32 {
            PERLIN.get([
                context.position.x as f64,
                context.position.y as f64,
                context.position.z as f64,
            ]) as f32
        }
    }

    pub struct DepthInstruction {}
    impl Instruction<f32> for DepthInstruction {
        fn process(&self, context: &SampleContext) -> f32 {
            context.depth
        }
    }
    pub struct MoistureInstruction {}
    impl Instruction<f32> for MoistureInstruction {
        fn process(&self, context: &SampleContext) -> f32 {
            context.moisture
        }
    }
    pub struct TemperatureInstruction {}
    impl Instruction<f32> for TemperatureInstruction {
        fn process(&self, context: &SampleContext) -> f32 {
            context.temperature
        }
    }
    pub struct DensityInstruction {}
    impl Instruction<f32> for DensityInstruction {
        fn process(&self, context: &SampleContext) -> f32 {
            context.density
        }
    }
}

pub struct SampleContext {
    position: IVec3,
    depth: f32,
    moisture: f32,
    temperature: f32,
    density: f32,
}

fn get_instruction_params(string: String) -> Vec<String> {
    let mut params = Vec::new();
    let mut current_param = String::new();
    let mut scope_depth = 0;
    for c in string.chars() {
        if c == '(' {
            scope_depth += 1;
        }
        if c == ')' {
            scope_depth -= 1;
        }
        if scope_depth == -1 {
            params.push(current_param.trim().to_string());
            break;
        }
        if scope_depth == 0 && c == ',' {
            params.push(current_param.trim().to_string());
            current_param = String::new();
        } else {
            current_param.push(c);
        }
    }
    params
}

fn build_bool_instruction(
    instruction: String,
    fields: &HashMap<&str, Arc<Box<dyn Instruction<f32>>>>,
) -> Arc<Box<dyn Instruction<bool>>> {
    println!("{instruction}");
    let (instruction_name, instruction_data) = instruction.split_once('(').unwrap();
    let params = get_instruction_params(instruction_data.to_string());
    match &instruction_name[..] {
        "Less" => {
            return Arc::new(Box::new(LessInstruction {
                val1: build_f32_instruction(params.get(0).unwrap().to_string(), fields),
                val2: build_f32_instruction(params.get(1).unwrap().to_string(), fields),
            }));
        }
        &_ => panic!("Unable to process given instruction: {}", instruction_name),
    }
}

fn build_f32_instruction(
    instruction: String,
    fields: &HashMap<&str, Arc<Box<dyn Instruction<f32>>>>,
) -> Arc<Box<dyn Instruction<f32>>> {
    let number = instruction.parse();

    if let Ok(number) = number {
        return Arc::new(Box::new(ConstInstruction { val: number }));
    }

    if fields.contains_key(&instruction[..]) {
        return Arc::clone(&fields.get(&instruction[..]).unwrap());
    }

    if !instruction.contains('(') {
        match &instruction[..] {
            "Depth" => {
                return Arc::new(Box::new(DepthInstruction {}));
            }
            "Moisture" => {
                return Arc::new(Box::new(MoistureInstruction {}));
            }
            "Temperature" => {
                return Arc::new(Box::new(TemperatureInstruction {}));
            }
            "Density" => {
                return Arc::new(Box::new(DensityInstruction {}));
            }
            &_ => panic!(
                "Constant variable '{}' was not found while constructing f32 instruction",
                instruction
            ),
        }
    }

    println!("{instruction}");
    let (instruction_name, instruction_data) = instruction.split_once('(').unwrap();
    println!("{instruction_data}");
    let params = get_instruction_params(instruction_data.to_string());
    params.iter().for_each(|v| println!("{v}"));
    match &instruction_name[..] {
        "If" => {
            return Arc::new(Box::new(IfInstruction {
                condition: build_bool_instruction(params.get(0).unwrap().to_string(), fields),
                val1: build_f32_instruction(params.get(1).unwrap().to_string(), fields),
                val2: build_f32_instruction(params.get(2).unwrap().to_string(), fields),
            }));
        }
        "Add" => {
            return Arc::new(Box::new(AddInstruction {
                val1: build_f32_instruction(params.get(0).unwrap().to_string(), fields),
                val2: build_f32_instruction(params.get(1).unwrap().to_string(), fields),
            }));
        }
        &_ => panic!(
            "Unable to process given instruction for type f32: {}",
            instruction_name
        ),
    }
}

fn build_voxel_type_instruction(
    instruction: String,
    fields: &HashMap<&str, Arc<Box<dyn Instruction<f32>>>>,
) -> Arc<Box<dyn Instruction<u16>>> {
    let (instruction_name, instruction_data) = instruction.split_once('(').unwrap();

    let params = get_instruction_params(instruction_data.to_string());
    match &instruction_name[..] {
        "If" => {
            return Arc::new(Box::new(IfInstruction {
                condition: build_bool_instruction(params.get(0).unwrap().to_string(), fields),
                val1: build_voxel_type_instruction(params.get(1).unwrap().to_string(), fields),
                val2: build_voxel_type_instruction(params.get(2).unwrap().to_string(), fields),
            }));
        }
        "Voxel" => {
            return Arc::new(Box::new(ConstInstruction {
                val: get_voxel_by_name(params.get(0).unwrap().to_string())
                    .unwrap()
                    .id,
            }))
        }
        &_ => panic!("Unable to process given instruction: {}", instruction_name),
    }
}

fn build_voxel_shape_instruction(
    instruction: String,
    fields: &HashMap<&str, Arc<Box<dyn Instruction<f32>>>>,
) -> Arc<Box<dyn Instruction<VoxelShape>>> {
    if !instruction.contains('(') {
        // Const value
        return Arc::new(Box::new(ConstInstruction {
            val: match &instruction[..] {
                "CUBE" => voxel_shape::CUBE,
                "SLAB" => voxel_shape::SLAB,
                &_ => panic!("Shape '{}' does is not defined", instruction),
            },
        }));
    }

    let (instruction_name, instruction_data) = instruction.split_once('(').unwrap();

    let params = get_instruction_params(instruction_data.to_string());
    match &instruction_name[..] {
        "If" => {
            return Arc::new(Box::new(IfInstruction {
                condition: build_bool_instruction(params.get(0).unwrap().to_string(), fields),
                val1: build_voxel_shape_instruction(params.get(1).unwrap().to_string(), fields),
                val2: build_voxel_shape_instruction(params.get(2).unwrap().to_string(), fields),
            }));
        }
        &_ => panic!("Unable to process given instruction: {}", instruction_name),
    }
}
