pub struct JvlmCompileOptions<FNM> where FNM: FunctionNameMapper {
    pub name_mapper: FNM
}

impl Default for JvlmCompileOptions<DefaultFunctionNameMapper> {
    fn default() -> Self {
        Self { name_mapper: Default::default() }
    }
}

pub trait FunctionNameMapper {
    /// Retrieves all information that can be elided from a function name.
    /// It mainly computes which class/classname this function should reside in.
    fn get_java_location(&self, c_name: &str) -> JavaFunctionLocation;
    /// Returns `Some` if this function is a special way to use the `new` bytecode operation.
    /// In this case, this function should return the name of the class which should be new'ed
    fn is_special_new_function(&self, c_name: &str) -> Option<String>;
    fn get_static_field_location(&self, c_name: &str) -> StaticFieldLocation;
}

pub struct StaticFieldLocation {
    pub class: String,
    pub name: String,
    pub extra_type_info: Option<String>,
}

#[derive(Debug)]
pub struct JavaFunctionLocation {
    pub class: String,
    pub name: String,
    pub external: bool,
    pub ty: FunctionType,
    pub extra_type_info: ExtraTypeInfo,
}

/// Represents extra info about which classes are used in the descriptor. This
/// information can't be represented in the llvm typesystem itself (it only contains plain
/// pointers, without much information. Any information that can be attached can't easily be
/// attached from C, Rust or other language). Thus, we express it in a mangled form inside the function name.
pub type ExtraTypeInfo = Option<Vec<String>>;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum FunctionType {
    Special,
    Virtual,
    Static,
}

#[derive(Default)]
pub struct DefaultFunctionNameMapper();

// jvlm_extern_new__ is especially special, and thus not defined here
const JVLM_SPECIAL_SYNTAX: &[(&'static str, FunctionType, bool)] = &[
    ("jvlm__", FunctionType::Static, false),
    ("jvlm_extern__", FunctionType::Static, true),
    ("jvlm_extern_invokespecial__", FunctionType::Special, true),
    ("jvlm_extern_invokevirtual__", FunctionType::Virtual, true),
];

impl DefaultFunctionNameMapper {
    fn demangle(&self, c_name: &str) -> String {
        // Within transliterated java names, we give characters some different meanings
        return c_name.replace("_", "/").replace("\u{022A}", "<").replace("\u{022B}", ">").replace("\u{022C}", "_")
    }

    fn split_typeinfo<'a>(&self, c_name: &'a str) -> (&'a str, Option<&'a str>) {
        if let Some(spl) = c_name.rsplit_once("$jvlm_param$") {
            return (spl.0, Some(spl.1));
        } else {
            return (c_name, None);
        }
    }

    fn parse_type_info(&self, info: &str) -> Vec<String> {
        info.split("\u{0229}").map(|p| self.demangle(p)).collect()
    }
}

impl FunctionNameMapper for DefaultFunctionNameMapper {
    fn get_java_location(&self, c_name: &str) -> JavaFunctionLocation {
        // Check if it starts with a magic string, any function like that will get placed in a specific
        // position.
        let (c_name, type_info) = self.split_typeinfo(c_name);
        let type_info = type_info.map(|i| self.parse_type_info(i));

        for syntax in JVLM_SPECIAL_SYNTAX {
            if let Some(name) = c_name.strip_prefix(syntax.0) {
                let e = self.demangle(name);
                let e = e.rsplit_once("/").unwrap();
                return JavaFunctionLocation {
                    class: e.0.to_owned(),
                    name: e.1.to_owned(),
                    external: syntax.2,
                    ty: syntax.1,
                    extra_type_info: type_info,
                };
            }
        }

        return JavaFunctionLocation {
            class: format!("jvlm/{}", c_name),
            name: c_name.to_string(),
            external: false,
            ty: FunctionType::Static,
            extra_type_info: type_info,
        };
    }
    
    fn is_special_new_function(&self, c_name: &str) -> Option<String> {
        if let Some(target) = c_name.strip_prefix("jvlm_extern_new__") {
            return Some(self.demangle(target));
        }
        return None;
    }
    
    fn get_static_field_location(&self, c_name: &str) -> StaticFieldLocation {
        let (c_name, type_info) = self.split_typeinfo(c_name);
        let type_info = type_info.map(|i| self.parse_type_info(i));
        let type_info = type_info.map(|i| i.into_iter().next().unwrap());

        if let Some(name) = c_name.strip_prefix("jvlm__") {
            let e = self.demangle(name);
            let e = e.rsplit_once("/").unwrap();
            return StaticFieldLocation {
                class: e.0.to_owned(),
                name: e.1.to_owned(),
                extra_type_info: type_info,
            };
        }

        return StaticFieldLocation {
            class: format!("jvlm/s/{}", c_name),
            name: c_name.to_string(),
            extra_type_info: type_info,
        };
    }
}