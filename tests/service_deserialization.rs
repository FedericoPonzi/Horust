use horust::Horust;
use std::path::PathBuf;

pub fn list_files<P: AsRef<std::path::Path>>(path: P) -> std::io::Result<Vec<std::path::PathBuf>> {
    let mut paths = std::fs::read_dir(path)?;
    paths.try_fold(vec![], |mut ret, p| match p {
        Ok(entry) => {
            ret.push(entry.path());
            Ok(ret)
        }
        Err(err) => Err(err),
    })
}

#[test]
fn should_deserialize() {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let services_path = base.join("example_services");
    let services = list_files(&services_path).unwrap().len();
    let horust = Horust::from_services_dir(&services_path).unwrap();
    assert_eq!(horust.services.len(), services);
}
