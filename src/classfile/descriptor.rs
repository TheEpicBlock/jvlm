use std::fmt::Display;

use super::JavaType;

pub enum DescriptorEntry {
    Byte,
    Char,
    Double,
    Float,
    Int,
    Long,
    Class(String),
    Short,
    Boolean,
    Array(Box<DescriptorEntry>),
}

pub type FieldDescriptor = DescriptorEntry;
pub struct MethodDescriptor(pub Vec<DescriptorEntry>, pub Option<DescriptorEntry>);

impl Display for DescriptorEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DescriptorEntry::Byte => write!(f, "B")?,
            DescriptorEntry::Char => write!(f, "C")?,
            DescriptorEntry::Double => write!(f, "D")?,
            DescriptorEntry::Float => write!(f, "F")?,
            DescriptorEntry::Int => write!(f, "I")?,
            DescriptorEntry::Long => write!(f, "J")?,
            DescriptorEntry::Class(c) => write!(f, "L{c};")?,
            DescriptorEntry::Short => write!(f, "S")?,
            DescriptorEntry::Boolean => write!(f, "Z")?,
            DescriptorEntry::Array(i) => write!(f, "[{i}")?,
        }
        Ok(())
    }
}

impl Display for MethodDescriptor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "(")?;
        for param in &self.0 {
            write!(f, "{}", param)?;
        }
        write!(f, ")")?;
        match &self.1 {
            Some(x) => write!(f, "{}", x)?,
            None => write!(f, "V")?,
        }
        Ok(())
    }
}

impl From<JavaType> for DescriptorEntry {
    fn from(value: JavaType) -> Self {
        match value {
            JavaType::Int => Self::Int,
            JavaType::Long => Self::Long,
            JavaType::Float => Self::Float,
            JavaType::Double => Self::Double,
            JavaType::Reference => todo!(),
        }
    }
}