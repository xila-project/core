//! Build-only cache validation and converter input discovery.

use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

pub fn digest(parts: &[&[u8]]) -> String {
    let mut hash = Sha256::new();
    for part in parts {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part);
    }
    format!("{:x}", hash.finalize())
}

fn hash_file(hash: &mut Sha256, path: &Path) -> io::Result<()> {
    let mut file = fs::File::open(path)?;
    hash.update(file.metadata()?.len().to_be_bytes());
    let mut bytes = [0; 16384];
    loop {
        let count = file.read(&mut bytes)?;
        if count == 0 {
            break;
        }
        hash.update(&bytes[..count]);
    }
    Ok(())
}

pub fn file_digest(path: &Path) -> io::Result<String> {
    let mut hash = Sha256::new();
    hash_file(&mut hash, path)?;
    Ok(format!("{:x}", hash.finalize()))
}

pub fn input_digest(paths: &[PathBuf]) -> io::Result<String> {
    let mut hash = Sha256::new();
    for path in paths.iter().collect::<BTreeSet<_>>() {
        let name = path.to_string_lossy();
        hash.update((name.len() as u64).to_be_bytes());
        hash.update(name.as_bytes());
        if path.is_file() {
            hash_file(&mut hash, path)?;
        }
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn remove(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

pub fn ensure_output(
    path: &Path,
    identity: &str,
    generate: impl FnOnce(&Path) -> io::Result<()>,
) -> io::Result<bool> {
    let marker = path.with_extension("c.json");
    let metadata = match fs::read(&marker) {
        Ok(bytes) => serde_json::from_slice::<serde_json::Value>(&bytes).ok(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    if let Some(metadata) = metadata {
        match fs::metadata(path) {
            Ok(file)
                if file.len() > 0
                    && metadata["version"] == 1
                    && metadata["identity"] == identity =>
            {
                if metadata["output"] == file_digest(path)? {
                    return Ok(false);
                }
            }
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
    }
    fs::create_dir_all(path.parent().expect("generated font parent"))?;
    remove(&marker)?;
    let temporary = path.with_extension("c.partial");
    let temporary_marker = path.with_extension("c.json.partial");
    let result = (|| {
        remove(&temporary)?;
        generate(&temporary)?;
        if fs::metadata(&temporary)?.len() == 0 {
            return Err(io::Error::other("generated font is empty"));
        }
        let output = file_digest(&temporary)?;
        fs::rename(&temporary, path)?;
        fs::write(
            &temporary_marker,
            serde_json::json!({"version": 1, "identity": identity, "output": output}).to_string(),
        )?;
        fs::rename(&temporary_marker, &marker)?;
        Ok(true)
    })();
    let cleanup = remove(&temporary).and_then(|()| remove(&temporary_marker));
    result.and_then(|changed| cleanup.map(|()| changed))
}

// Resolve npm's nearest node_modules package from its owning package, including
// nested/bundled and hoisted dependencies. No JavaScript helper is needed.
pub fn converter_inputs(root: &Path) -> io::Result<Vec<PathBuf>> {
    fn walk(path: &Path, inputs: &mut BTreeSet<PathBuf>) -> io::Result<()> {
        let path = fs::canonicalize(path)?;
        if !inputs.insert(path.clone()) || !path.is_dir() {
            return Ok(());
        }
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            if entry.file_name() != ".git" {
                walk(&entry.path(), inputs)?;
            }
        }
        Ok(())
    }
    fn visit(
        root: &Path,
        visited: &mut BTreeSet<PathBuf>,
        inputs: &mut BTreeSet<PathBuf>,
    ) -> io::Result<()> {
        let root = fs::canonicalize(root)?;
        if !visited.insert(root.clone()) {
            return Ok(());
        }
        walk(&root, inputs)?;
        let manifest = root.join("package.json");
        let package: serde_json::Value = serde_json::from_slice(&fs::read(&manifest)?)
            .map_err(|error| io::Error::other(format!("{}: {error}", manifest.display())))?;
        for field in ["dependencies", "optionalDependencies", "peerDependencies"] {
            if let Some(dependencies) = package[field].as_object() {
                for name in dependencies.keys() {
                    let found = root
                        .ancestors()
                        .map(|parent| parent.join("node_modules").join(name))
                        .find(|candidate| candidate.join("package.json").is_file());
                    if let Some(dependency) = found {
                        visit(&dependency, visited, inputs)?;
                    } else if field == "dependencies"
                        && package["optionalDependencies"].get(name).is_none()
                    {
                        return Err(io::Error::other(format!(
                            "Missing converter dependency {name} from {}; run npm ci",
                            root.display()
                        )));
                    }
                }
            }
        }
        Ok(())
    }
    let mut inputs = BTreeSet::new();
    visit(root, &mut BTreeSet::new(), &mut inputs)?;
    Ok(inputs.into_iter().collect())
}
