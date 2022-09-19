use std::path::PathBuf;

pub fn list_files<P: AsRef<std::path::Path>>(path: P) -> std::io::Result<Vec<PathBuf>> {
    let mut paths = std::fs::read_dir(path)?;
    paths.try_fold(vec![], |mut ret, p| match p {
        Ok(entry) => {
            ret.push(entry.path());
            Ok(ret)
        }
        Err(err) => Err(err),
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use horust::Horust;

    use crate::list_files;

    #[test]
    fn should_deserialize() {
        // TODO: this shouldn't be an integration test, but rather a unit test.
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let services_path = base.join("example_services");
        let services = list_files(&services_path).unwrap().len();
        let horust = Horust::from_services_dirs(&[services_path]).unwrap();
        assert_eq!(horust.get_services().len(), services);
    }
}
