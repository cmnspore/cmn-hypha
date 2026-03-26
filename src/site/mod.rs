use std::fs;
use std::path::PathBuf;

pub fn validate_site_domain_path(domain: &str) -> Result<(), String> {
    if domain.is_empty() {
        return Err("Domain must not be empty".to_string());
    }
    if domain.chars().any(|c| c.is_control()) {
        return Err(format!(
            "Invalid domain '{}': contains control characters",
            domain
        ));
    }

    let mut components = std::path::Path::new(domain).components();
    let single_normal_component =
        matches!(components.next(), Some(std::path::Component::Normal(_)))
            && components.next().is_none();
    if !single_normal_component {
        return Err(format!(
            "Invalid domain '{}': must be a single path segment",
            domain
        ));
    }

    Ok(())
}

/// Get the CMN base directory (~/.cmn or CMN_HOME)
pub fn get_cmn_home() -> PathBuf {
    if let Ok(home) = std::env::var("CMN_HOME") {
        PathBuf::from(home)
    } else {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".cmn")
    }
}

/// Get the base directory for all mycelium sites
pub fn get_sites_dir() -> PathBuf {
    get_cmn_home().join("mycelium")
}

/// Get the directory for a specific domain
pub fn get_site_dir(domain: &str) -> PathBuf {
    get_sites_dir().join(domain)
}

/// Site directory structure
pub struct SiteDir {
    pub root: PathBuf,
    pub keys: PathBuf,
    pub public: PathBuf,
}

impl SiteDir {
    /// Create SiteDir with default path (~/.cmn/mycelium/<domain>)
    pub fn new(domain: &str) -> Self {
        Self::with_path(get_site_dir(domain))
    }

    /// Create SiteDir with custom path
    pub fn with_path(root: PathBuf) -> Self {
        Self {
            keys: root.join("keys"),
            public: root.join("public"),
            root,
        }
    }

    /// Create SiteDir from domain and optional custom path
    pub fn from_args(domain: &str, custom_path: Option<&str>) -> Self {
        match custom_path {
            Some(path) => Self::with_path(PathBuf::from(path)),
            None => Self::new(domain),
        }
    }

    pub fn private_key_path(&self) -> PathBuf {
        self.keys.join("private.pem")
    }

    pub fn public_key_path(&self) -> PathBuf {
        self.keys.join("public.pem")
    }

    pub fn cmn_json_path(&self) -> PathBuf {
        self.public.join(".well-known").join("cmn.json")
    }

    pub fn cmn_protocol_dir(&self) -> PathBuf {
        self.public.join("cmn")
    }

    pub fn mycelium_dir(&self) -> PathBuf {
        self.cmn_protocol_dir().join("mycelium")
    }

    pub fn spores_dir(&self) -> PathBuf {
        self.cmn_protocol_dir().join("spore")
    }

    pub fn archive_dir(&self) -> PathBuf {
        self.cmn_protocol_dir().join("archive")
    }

    pub fn taste_dir(&self) -> PathBuf {
        self.cmn_protocol_dir().join("taste")
    }

    pub fn exists(&self) -> bool {
        self.root.exists()
    }

    pub fn create_dirs(&self) -> anyhow::Result<()> {
        fs::create_dir_all(&self.keys)?;
        // Restrict keys dir to owner-only access
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.keys, fs::Permissions::from_mode(0o700))?;
        }
        fs::create_dir_all(&self.public)?;
        fs::create_dir_all(self.mycelium_dir())?;
        fs::create_dir_all(self.spores_dir())?;
        fs::create_dir_all(self.archive_dir())?;
        fs::create_dir_all(self.taste_dir())?;
        Ok(())
    }

    /// Full endpoint URL templates for cmn.json capsule entries
    pub fn endpoints(base_url: &str) -> Vec<substrate::model::CmnEndpoint> {
        vec![
            substrate::model::CmnEndpoint {
                kind: "mycelium".to_string(),
                url: format!("{}/cmn/mycelium/{{hash}}.json", base_url),
                hash: String::new(),
                hashes: vec![],
                format: None,
                delta_url: None,
                protocol_version: None,
            },
            substrate::model::CmnEndpoint {
                kind: "spore".to_string(),
                url: format!("{}/cmn/spore/{{hash}}.json", base_url),
                hash: String::new(),
                hashes: vec![],
                format: None,
                delta_url: None,
                protocol_version: None,
            },
            substrate::model::CmnEndpoint {
                kind: "archive".to_string(),
                url: format!("{}/cmn/archive/{{hash}}.tar.zst", base_url),
                hash: String::new(),
                hashes: vec![],
                format: Some("tar+zstd".to_string()),
                delta_url: Some(format!(
                    "{}/cmn/archive/{{hash}}.from.{{old_hash}}.tar.zst",
                    base_url
                )),
                protocol_version: None,
            },
            substrate::model::CmnEndpoint {
                kind: "taste".to_string(),
                url: format!("{}/cmn/taste/{{hash}}.json", base_url),
                hash: String::new(),
                hashes: vec![],
                format: None,
                delta_url: None,
                protocol_version: None,
            },
        ]
    }
}

/// List all configured domains
pub fn list_domains() -> Vec<String> {
    let sites_dir = get_sites_dir();
    let mut domains = Vec::new();

    if let Ok(entries) = fs::read_dir(&sites_dir) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                if let Some(name) = entry.file_name().to_str() {
                    // Check if it has keys
                    let site = SiteDir::new(name);
                    if site.private_key_path().exists() {
                        domains.push(name.to_string());
                    }
                }
            }
        }
    }

    domains
}
