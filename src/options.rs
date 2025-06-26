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

pub struct JavaFunctionLocation {
    pub class: String,
    pub name: String,
}

#[derive(Default)]
pub struct DefaultFunctionNameMapper();

impl FunctionNameMapper for DefaultFunctionNameMapper {
    fn get_java_location(&self, c_name: &str) -> JavaFunctionLocation {
        return JavaFunctionLocation {
            class: format!("jvlm/{}", c_name),
            name: c_name.to_string()
        };
    }
}