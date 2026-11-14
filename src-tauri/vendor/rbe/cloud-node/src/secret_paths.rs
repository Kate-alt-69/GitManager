//! Secret and credential path exclusions shared by archive clients.

use std::path::Path;

/// Exclude common credentials and private-key material from repository archives by default.
/// Environment templates/examples remain available as safe placeholders.
pub fn is_default_secret_path(relative: &Path) -> bool {
    let components = relative.components()
        .filter_map(|component| component.as_os_str().to_str())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
    if components.is_empty() {
        return false;
    }

    const SECRET_DIRS: &[&str] = &[
        ".aws", ".azure", ".docker", ".gnupg", ".kube", ".secrets", ".ssh", "secrets",
    ];
    if components.iter().any(|part| SECRET_DIRS.contains(&part.as_str())) {
        return true;
    }
    let normalized = components.join("/");
    const SECRET_PATHS: &[&str] = &[
        ".config/azure",
        ".config/gcloud",
        ".config/gh",
        ".config/containers",
        ".terraform.d",
    ];
    if SECRET_PATHS.iter().any(|prefix| {
        normalized == *prefix || normalized.starts_with(&format!("{prefix}/"))
    }) {
        return true;
    }

    let name = components.last().map(String::as_str).unwrap_or_default();
    if name == ".env" || (name.starts_with(".env.") && !matches!(
        name, ".env.example" | ".env.sample" | ".env.template" | ".env.dist"
    )) {
        return true;
    }
    if matches!(name,
        "id_rsa" | "id_dsa" | "id_ecdsa" | "id_ed25519" |
        "credentials" | "credentials.json" | "secrets.json" | "secrets.yaml" |
        "secrets.yml" | "service-account.json" | "auth.json" |
        ".npmrc" | ".pypirc" | ".netrc" | "_netrc" | ".git-credentials" |
        ".authinfo" | ".pgpass" | ".my.cnf" | ".htpasswd" |
        ".terraformrc" | "dockerconfigjson"
    ) || (name.starts_with("service-account-") && name.ends_with(".json"))
        || name.starts_with("terraform.tfstate")
    {
        return true;
    }
    matches!(
        Path::new(name).extension().and_then(|value| value.to_str()).map(str::to_ascii_lowercase).as_deref(),
        Some("pem" | "key" | "p12" | "pfx" | "ppk" | "keystore" | "jks")
    )
}

#[cfg(test)]
mod tests {
    use super::is_default_secret_path;
    use std::path::Path;

    #[test]
    fn excludes_credentials_but_keeps_environment_templates() {
        for path in [
            ".env",
            ".env.production",
            "config/.env.local",
            "certs/signing.pfx",
            ".ssh/id_ed25519",
            "secrets/service-account.json",
            ".npmrc",
            "project/.netrc",
            ".docker/config.json",
            ".kube/config",
            ".config/gcloud/application_default_credentials.json",
            ".config/gh/hosts.yml",
            "terraform.tfstate.backup",
        ] {
            assert!(is_default_secret_path(Path::new(path)), "{path}");
        }

        for path in [
            ".env.example",
            ".env.sample",
            "src/config.rs",
            "docs/.env.example",
        ] {
            assert!(!is_default_secret_path(Path::new(path)), "{path}");
        }
    }
}
