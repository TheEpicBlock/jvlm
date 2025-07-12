use std::{io::{Read, Seek, Write}, iter::Zip};

use zip::{result::ZipError, ZipArchive, ZipWriter};

pub fn link(source_jars: impl Iterator<Item = impl Read+Seek>, out: impl Write+Seek) -> Result<(), ZipError> {
    let mut out_jar = ZipWriter::new(out);
    for jar_stream in source_jars {
        let zip = ZipArchive::new(jar_stream)?;
        out_jar.merge_archive(zip)?;
    }
    out_jar.finish()?;

    Ok(())
}