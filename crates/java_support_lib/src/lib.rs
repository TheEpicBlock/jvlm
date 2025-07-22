
struct BuiltinJavaCode {
    /// The name of the class (stored as a binary internal name, eg `java/lang/Thread`)
    pub name: &'static str,
    /// The bytes forming the classfile
    pub class: &'static [u8],
}

include!(concat!(env!("OUT_DIR"), "/java_compiled.rs"));
