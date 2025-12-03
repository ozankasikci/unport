use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// Detected framework and how to inject port
#[derive(Debug)]
pub struct Detection {
    pub framework: String,
    pub start_command: String,
    pub port_strategy: PortStrategy,
}

#[derive(Debug)]
pub enum PortStrategy {
    /// Set PORT environment variable
    EnvVar(String),
    /// Append --port flag to command
    CliFlag(String),
}

#[derive(Deserialize)]
struct PackageJson {
    scripts: Option<HashMap<String, String>>,
    dependencies: Option<HashMap<String, serde_json::Value>>,
    #[serde(rename = "devDependencies")]
    dev_dependencies: Option<HashMap<String, serde_json::Value>>,
}

/// Detect framework from project directory
pub fn detect(dir: &Path) -> Result<Detection> {
    // Check for package.json (Node.js projects)
    let package_json_path = dir.join("package.json");
    if package_json_path.exists() {
        let content = std::fs::read_to_string(&package_json_path)?;
        let package: PackageJson = serde_json::from_str(&content)?;

        let all_deps = merge_deps(&package.dependencies, &package.dev_dependencies);
        let scripts = package.scripts.unwrap_or_default();
        let dev_script = scripts.get("dev").cloned().unwrap_or_default();

        // Check for specific frameworks
        if all_deps.contains_key("next") {
            return Ok(Detection {
                framework: "Next.js".into(),
                start_command: "npm run dev".into(),
                port_strategy: PortStrategy::EnvVar("PORT".into()),
            });
        }

        if all_deps.contains_key("vite") || dev_script.contains("vite") {
            return Ok(Detection {
                framework: "Vite".into(),
                start_command: "npm run dev".into(),
                port_strategy: PortStrategy::CliFlag("--port".into()),
            });
        }

        if all_deps.contains_key("react-scripts") {
            return Ok(Detection {
                framework: "Create React App".into(),
                start_command: "npm start".into(),
                port_strategy: PortStrategy::EnvVar("PORT".into()),
            });
        }

        if all_deps.contains_key("@remix-run/dev") {
            return Ok(Detection {
                framework: "Remix".into(),
                start_command: "npm run dev".into(),
                port_strategy: PortStrategy::EnvVar("PORT".into()),
            });
        }

        if all_deps.contains_key("nuxt") {
            return Ok(Detection {
                framework: "Nuxt".into(),
                start_command: "npm run dev".into(),
                port_strategy: PortStrategy::EnvVar("PORT".into()),
            });
        }

        if all_deps.contains_key("@nestjs/core") {
            return Ok(Detection {
                framework: "NestJS".into(),
                start_command: "npm run start:dev".into(),
                port_strategy: PortStrategy::EnvVar("PORT".into()),
            });
        }

        if all_deps.contains_key("fastify") {
            return Ok(Detection {
                framework: "Fastify".into(),
                start_command: "npm run dev".into(),
                port_strategy: PortStrategy::EnvVar("PORT".into()),
            });
        }

        if all_deps.contains_key("express") {
            return Ok(Detection {
                framework: "Express".into(),
                start_command: "npm run dev".into(),
                port_strategy: PortStrategy::EnvVar("PORT".into()),
            });
        }

        // Generic Node.js project
        let start_cmd = if scripts.contains_key("dev") {
            "npm run dev"
        } else if scripts.contains_key("start") {
            "npm start"
        } else {
            "npm run dev"
        };

        return Ok(Detection {
            framework: "Node.js".into(),
            start_command: start_cmd.into(),
            port_strategy: PortStrategy::EnvVar("PORT".into()),
        });
    }

    // Check for Rails (Gemfile)
    if dir.join("Gemfile").exists() {
        return Ok(Detection {
            framework: "Rails".into(),
            start_command: "rails server".into(),
            port_strategy: PortStrategy::CliFlag("-p".into()),
        });
    }

    // Check for Django (manage.py)
    if dir.join("manage.py").exists() {
        return Ok(Detection {
            framework: "Django".into(),
            start_command: "python manage.py runserver".into(),
            port_strategy: PortStrategy::CliFlag("0.0.0.0:".into()), // Special case: appends port directly
        });
    }

    // Check for Go
    if dir.join("go.mod").exists() {
        return Ok(Detection {
            framework: "Go".into(),
            start_command: "go run .".into(),
            port_strategy: PortStrategy::EnvVar("PORT".into()),
        });
    }

    // Default fallback
    Ok(Detection {
        framework: "Unknown".into(),
        start_command: "npm run dev".into(),
        port_strategy: PortStrategy::EnvVar("PORT".into()),
    })
}

fn merge_deps(
    deps: &Option<HashMap<String, serde_json::Value>>,
    dev_deps: &Option<HashMap<String, serde_json::Value>>,
) -> HashMap<String, serde_json::Value> {
    let mut all = HashMap::new();
    if let Some(d) = deps {
        all.extend(d.clone());
    }
    if let Some(d) = dev_deps {
        all.extend(d.clone());
    }
    all
}
