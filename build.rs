fn main() {
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let version = std::process::Command::new(&rustc)
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "rustc (unknown)".to_string());

    println!("cargo:rustc-env=RUSTC_VERSION={}", version);

    // Extract Axum and Tokio versions dynamically from Cargo.lock or Cargo.toml
    let cargo_lock = std::fs::read_to_string("Cargo.lock").unwrap_or_default();
    let cargo_toml = std::fs::read_to_string("Cargo.toml").unwrap_or_default();

    let axum_version = get_crate_version(&cargo_lock, &cargo_toml, "axum")
        .unwrap_or_else(|| "0.8".to_string());
    let tokio_version = get_crate_version(&cargo_lock, &cargo_toml, "tokio")
        .unwrap_or_else(|| "1.40".to_string());
    let sqlx_version = get_crate_version(&cargo_lock, &cargo_toml, "sqlx")
        .unwrap_or_else(|| "0.9".to_string());

    println!("cargo:rustc-env=AXUM_VERSION={}", axum_version);
    println!("cargo:rustc-env=TOKIO_VERSION={}", tokio_version);
    println!("cargo:rustc-env=SQLX_VERSION={}", sqlx_version);

    // Extract Vite version dynamically from web/package-lock.json or web/package.json
    let vite_version = get_vite_version();
    println!("cargo:rustc-env=VITE_VERSION={}", vite_version);

    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=Cargo.lock");
    println!("cargo:rerun-if-changed=web/package-lock.json");
    println!("cargo:rerun-if-changed=web/package.json");
}

fn get_crate_version(lock_content: &str, toml_content: &str, pkg_name: &str) -> Option<String> {
    if !lock_content.is_empty() {
        let mut in_pkg = false;
        for line in lock_content.lines() {
            let trimmed = line.trim();
            if trimmed == "[[package]]" {
                in_pkg = false;
            } else if trimmed == format!("name = \"{}\"", pkg_name) {
                in_pkg = true;
            } else if in_pkg && trimmed.starts_with("version = \"") {
                if let Some(ver) = trimmed.strip_prefix("version = \"").and_then(|s| s.strip_suffix('"')) {
                    return Some(ver.to_string());
                }
            }
        }
    }

    if !toml_content.is_empty() {
        for line in toml_content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with(&format!("{} ", pkg_name)) || trimmed.starts_with(&format!("{}=", pkg_name)) {
                if let Some(pos) = trimmed.find("version = \"") {
                    let rest = &trimmed[pos + 11..];
                    if let Some(end) = rest.find('"') {
                        return Some(rest[..end].to_string());
                    }
                } else if let Some(pos) = trimmed.find('"') {
                    let rest = &trimmed[pos + 1..];
                    if let Some(end) = rest.find('"') {
                        return Some(rest[..end].to_string());
                    }
                }
            }
        }
    }

    None
}

fn get_vite_version() -> String {
    if let Ok(lock_str) = std::fs::read_to_string("web/package-lock.json") {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&lock_str) {
            if let Some(ver) = val
                .get("packages")
                .and_then(|p| p.get("node_modules/vite"))
                .and_then(|v| v.get("version"))
                .and_then(|v| v.as_str())
            {
                return ver.to_string();
            }
            if let Some(ver) = val
                .get("dependencies")
                .and_then(|d| d.get("vite"))
                .and_then(|v| v.get("version"))
                .and_then(|v| v.as_str())
            {
                return ver.to_string();
            }
        }
    }

    if let Ok(pkg_str) = std::fs::read_to_string("web/package.json") {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&pkg_str) {
            if let Some(ver) = val
                .get("devDependencies")
                .and_then(|d| d.get("vite"))
                .and_then(|v| v.as_str())
            {
                return ver.trim_start_matches('^').trim_start_matches('~').to_string();
            }
        }
    }

    "8.3.5".to_string()
}
