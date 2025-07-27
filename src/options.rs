pub struct JvlmCompileOptions<FNM> where FNM: FunctionNameMapper {
    pub name_mapper: FNM
}

impl Default for JvlmCompileOptions<DefaultFunctionNameMapper> {
    fn default() -> Self {
        Self { name_mapper: Default::default() }
    }
}

pub trait FunctionNameMapper {
    fn get_java_location(&self, c_name: &str) -> JavaFunctionLocation;
}

#[derive(Debug)]
pub struct JavaFunctionLocation {
    pub class: String,
    pub name: String,
}

#[derive(Default)]
pub struct DefaultFunctionNameMapper();

impl FunctionNameMapper for DefaultFunctionNameMapper {
    fn get_java_location(&self, c_name: &str) -> JavaFunctionLocation {
        // Check if it starts with a magic string, any function like that will get placed in a specific
        // position. This can also be used with external function declarations
        if let Some(target_name) = c_name.strip_prefix("jvlm_extern__") {
            let split: Vec<_> = target_name.split("_").collect();
            return JavaFunctionLocation {
                class: split[0..split.len()-1].join("/"),
                name: split.last().unwrap().to_string(),
            };
        }

        return JavaFunctionLocation {
            class: format!("jvlm/{}", c_name),
            name: c_name.to_string(),
        };
    }
}