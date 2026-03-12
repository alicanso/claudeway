fn main() {
    use std::process::Command;

    let dashboard_dir = std::path::Path::new("dashboard");

    if !dashboard_dir.join("package.json").exists() {
        println!("cargo:warning=Dashboard source not found at dashboard/. Skipping frontend build.");
        return;
    }

    let dist_dir = dashboard_dir.join("dist");
    let src_dir = dashboard_dir.join("src");

    if dist_dir.exists() {
        let dist_mtime = std::fs::metadata(&dist_dir)
            .and_then(|m| m.modified())
            .ok();

        let needs_rebuild = walkdir(&src_dir)
            .map(|src_mtime| match dist_mtime {
                Some(d) => src_mtime > d,
                None => true,
            })
            .unwrap_or(true);

        if !needs_rebuild {
            println!("cargo:warning=Dashboard dist is up to date, skipping build.");
            return;
        }
    }

    let status = match Command::new("npm")
        .arg("install")
        .current_dir(dashboard_dir)
        .status()
    {
        Ok(s) => s,
        Err(_) => {
            println!("cargo:warning=npm not found, skipping dashboard build.");
            return;
        }
    };

    if !status.success() {
        panic!("npm install failed");
    }

    let status = match Command::new("npm")
        .args(["run", "build"])
        .current_dir(dashboard_dir)
        .status()
    {
        Ok(s) => s,
        Err(_) => {
            println!("cargo:warning=npm not found, skipping dashboard build.");
            return;
        }
    };

    if !status.success() {
        panic!("npm run build failed");
    }

    println!("cargo:rerun-if-changed=dashboard/src");
    println!("cargo:rerun-if-changed=dashboard/package.json");
    println!("cargo:rerun-if-changed=dashboard/vite.config.ts");
}

fn walkdir(dir: &std::path::Path) -> Option<std::time::SystemTime> {
    let mut latest: Option<std::time::SystemTime> = None;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(t) = walkdir(&path) {
                    latest = Some(latest.map_or(t, |l: std::time::SystemTime| l.max(t)));
                }
            } else if let Ok(meta) = std::fs::metadata(&path)
                && let Ok(mtime) = meta.modified()
            {
                latest = Some(latest.map_or(mtime, |l: std::time::SystemTime| l.max(mtime)));
            }
        }
    }
    latest
}
