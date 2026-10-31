//! NuGet packages (`.nupkg`): a zip of the files plus the Open Packaging Conventions parts that NuGet reads.
//!
//! The package holds `ID.nuspec` (the metadata, see <https://learn.microsoft.com/en-us/nuget/reference/nuspec>),
//! `_rels/.rels` (relationships to the nuspec and to the core properties), `[Content_Types].xml` and a
//! `package/services/metadata/core-properties/HASH.psmdcp` part. The layout, namespaces and element order follow what
//! `dotnet pack` writes (NuGet.Client, `NuGet.Build.Tasks.Pack`, observed on `mcr.microsoft.com/dotnet/sdk:8.0`).
//! NuGet has no install scripts, no architecture, no configuration files and no distro dependencies: those manifest
//! fields are warned about and not written.

use anyhow::{Context, Result, bail};
use pkgsmith_plugin::hash::hex;
use pkgsmith_plugin::version::Version;
use pkgsmith_plugin::{Data, Host, Package};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use zip::write::SimpleFileOptions;

/// The `nupkg:` section of the manifest.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    /// The top folder in the package that receives the files. Default `tools`.
    #[serde(default)]
    folder: Folder,
    /// Removed from the front of each file path before the path goes below `folder`. Default: none.
    prefix: Option<String>,
    /// The NuGet dependencies, grouped by target framework in the nuspec. Default: none.
    #[serde(default)]
    dependencies: Vec<Dependency>,
}

/// A top folder that NuGet packages use for their files.
#[derive(Debug, Default, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Folder {
    Lib,
    Content,
    ContentFiles,
    Build,
    #[default]
    Tools,
    Runtimes,
}

impl Folder {
    fn name(self) -> &'static str {
        match self {
            Folder::Lib => "lib",
            Folder::Content => "content",
            Folder::ContentFiles => "contentFiles",
            Folder::Build => "build",
            Folder::Tools => "tools",
            Folder::Runtimes => "runtimes",
        }
    }
}

/// One `<dependency>` of the nuspec.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Dependency {
    /// The package ID, for example `Newtonsoft.Json`.
    id: String,
    /// A NuGet version or range, for example `13.0.1` (that version or higher) or `[13.0,14.0)`. Default: any version.
    version: Option<String>,
    /// The framework the dependency applies to, for example `net8.0`. Default: every framework.
    target_framework: Option<String>,
}

/// `ID.VERSION.nupkg`, as `dotnet pack` names it.
pub fn file_name(package: &Package) -> Result<String> {
    let id = package_id(package)?;
    let version = Version::parse(&package.version)?.plain();
    Ok(format!("{id}.{version}.nupkg"))
}

/// NuGet IDs are letters, digits, `.`, `-` and `_`.
fn package_id(package: &Package) -> Result<&str> {
    let valid = !package.name.is_empty()
        && package
            .name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    if !valid {
        bail!(
            "the NuGet id {:?} may only hold letters, digits, '.', '-' and '_'",
            package.name
        );
    }
    Ok(&package.name)
}

/// Escapes text for an XML element or attribute.
fn escape(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '&' => "&amp;".to_string(),
            '<' => "&lt;".to_string(),
            '>' => "&gt;".to_string(),
            '"' => "&quot;".to_string(),
            '\'' => "&apos;".to_string(),
            other => other.to_string(),
        })
        .collect()
}

/// The path of a file inside the package: `folder/` plus the path with the prefix removed.
fn member_path(folder: Folder, prefix: &str, path: &str) -> Result<String> {
    let path = path.trim_start_matches('/');
    let rest = if prefix.is_empty() {
        path
    } else {
        path.strip_prefix(prefix)
            .and_then(|rest| rest.strip_prefix('/'))
            .with_context(|| format!("the file {path:?} is not below the prefix {prefix:?}"))?
    };
    if rest.is_empty() || rest.split('/').any(|part| part.is_empty() || part == "..") {
        bail!("the file {path:?} has no usable path below the prefix {prefix:?}");
    }
    Ok(format!("{}/{rest}", folder.name()))
}

/// The `<dependencies>` element, one `<group>` per target framework, or nothing when there are none.
fn dependencies_xml(dependencies: &[Dependency]) -> String {
    if dependencies.is_empty() {
        return String::new();
    }
    let mut groups: BTreeMap<Option<&str>, Vec<&Dependency>> = BTreeMap::new();
    for dependency in dependencies {
        groups
            .entry(dependency.target_framework.as_deref())
            .or_default()
            .push(dependency);
    }
    let mut xml = String::from("    <dependencies>\n");
    for (framework, members) in groups {
        match framework {
            Some(framework) => xml.push_str(&format!(
                "      <group targetFramework=\"{}\">\n",
                escape(framework)
            )),
            None => xml.push_str("      <group>\n"),
        }
        for dependency in members {
            xml.push_str(&format!(
                "        <dependency id=\"{}\"",
                escape(&dependency.id)
            ));
            if let Some(version) = &dependency.version {
                xml.push_str(&format!(" version=\"{}\"", escape(version)));
            }
            xml.push_str(" />\n");
        }
        xml.push_str("      </group>\n");
    }
    xml.push_str("    </dependencies>\n");
    xml
}

/// The nuspec: the metadata of the package. Element order follows the `dotnet pack` output.
fn nuspec(
    package: &Package,
    id: &str,
    version: &str,
    dependencies: &[Dependency],
) -> Result<String> {
    let authors = package
        .maintainer
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .with_context(|| "NuGet needs <authors>, so the manifest must name a maintainer")?;
    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
    xml.push_str("<package xmlns=\"http://schemas.microsoft.com/packaging/2012/06/nuspec.xsd\">\n");
    xml.push_str("  <metadata>\n");
    xml.push_str(&format!("    <id>{}</id>\n", escape(id)));
    xml.push_str(&format!("    <version>{}</version>\n", escape(version)));
    xml.push_str(&format!("    <authors>{}</authors>\n", escape(authors)));
    if let Some(license) = package
        .license
        .as_deref()
        .map(str::trim)
        .filter(|l| !l.is_empty())
    {
        xml.push_str(&format!(
            "    <license type=\"expression\">{}</license>\n",
            escape(license)
        ));
    }
    if let Some(homepage) = package
        .homepage
        .as_deref()
        .map(str::trim)
        .filter(|h| !h.is_empty())
    {
        xml.push_str(&format!(
            "    <projectUrl>{}</projectUrl>\n",
            escape(homepage)
        ));
    }
    xml.push_str(&format!(
        "    <description>{}</description>\n",
        escape(package.description.trim())
    ));
    xml.push_str(&dependencies_xml(dependencies));
    xml.push_str("  </metadata>\n");
    xml.push_str("</package>\n");
    Ok(xml)
}

/// The core properties part. NuGet reads the identity and description from it.
fn core_properties(id: &str, version: &str, authors: &str, description: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <coreProperties xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" \
         xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" \
         xmlns=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\">\n\
         \x20 <dc:creator>{}</dc:creator>\n\
         \x20 <dc:description>{}</dc:description>\n\
         \x20 <dc:identifier>{}</dc:identifier>\n\
         \x20 <version>{}</version>\n\
         \x20 <keywords></keywords>\n\
         \x20 <lastModifiedBy>pkgsmith</lastModifiedBy>\n\
         </coreProperties>\n",
        escape(authors),
        escape(description),
        escape(id),
        escape(version),
    )
}

/// The package relationships: the nuspec and the core properties.
fn relationships(id: &str, psmdcp: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\n\
         \x20 <Relationship Type=\"http://schemas.microsoft.com/packaging/2010/07/manifest\" Target=\"/{}.nuspec\" Id=\"R1\" />\n\
         \x20 <Relationship Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"/package/services/metadata/core-properties/{psmdcp}.psmdcp\" Id=\"R2\" />\n\
         </Relationships>\n",
        escape(id),
    )
}

/// The content types. Parts with an extension get a `Default`, parts without one an `Override`. NuGet uses
/// `application/octet` for the files and the content types of the OPC for the rest.
fn content_types(members: &[String]) -> String {
    let mut defaults: BTreeMap<&str, &str> = BTreeMap::new();
    defaults.insert(
        "rels",
        "application/vnd.openxmlformats-package.relationships+xml",
    );
    defaults.insert(
        "psmdcp",
        "application/vnd.openxmlformats-package.core-properties+xml",
    );
    defaults.insert("nuspec", "application/octet");
    let mut overrides = Vec::new();
    for member in members {
        let file_name = member.rsplit('/').next().unwrap_or(member);
        match file_name.rsplit_once('.') {
            Some((stem, extension)) if !stem.is_empty() => {
                defaults.entry(extension).or_insert("application/octet");
            }
            _ => overrides.push(format!("/{member}")),
        }
    }
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\n",
    );
    for (extension, content_type) in defaults {
        xml.push_str(&format!(
            "  <Default Extension=\"{}\" ContentType=\"{content_type}\" />\n",
            escape(extension)
        ));
    }
    for part in overrides {
        xml.push_str(&format!(
            "  <Override PartName=\"{}\" ContentType=\"application/octet\" />\n",
            escape(&part)
        ));
    }
    xml.push_str("</Types>\n");
    xml
}

/// Civil date and time of a Unix time, in UTC. A zip cannot hold a date before 1980, so earlier times become 1980.
fn civil(seconds: u64) -> (u16, u8, u8, u8, u8, u8) {
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    // Days since 1970-01-01 to a calendar date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u8;
    let month_wide = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let month = month_wide as u8;
    let year = (year_of_era + era * 400 + i64::from(month <= 2)) as u16;
    if year < 1980 {
        return (1980, 1, 1, 0, 0, 0);
    }
    (
        year,
        month,
        day,
        (rest / 3600) as u8,
        (rest % 3600 / 60) as u8,
        (rest % 60) as u8,
    )
}

/// Writes the warnings for the manifest fields that NuGet cannot express.
fn warn_unexpressed(package: &Package, host: &Host) {
    if package.arch != "all" {
        host.warn(&format!(
            "NuGet packages have no architecture; the {} binaries go in the package unlabelled (use runtimes/<rid>/ for per-platform files)",
            package.arch
        ));
    }
    let scripts = &package.scripts;
    if scripts.pre_install.is_some()
        || scripts.post_install.is_some()
        || scripts.pre_remove.is_some()
        || scripts.post_remove.is_some()
    {
        host.warn(
            "NuGet has no install or removal scripts (tools/install.ps1 is only read by packages.config projects); the scripts are not written",
        );
    }
    for (field, values) in [
        ("depends", &package.depends),
        ("recommends", &package.recommends),
        ("suggests", &package.suggests),
        ("conflicts", &package.conflicts),
        ("replaces", &package.replaces),
        ("provides", &package.provides),
    ] {
        if !values.is_empty() {
            host.warn(&format!(
                "the manifest {field} ({}) is not written: NuGet dependencies come from the nupkg dependencies option",
                values.join(", ")
            ));
        }
    }
}

pub fn build(package: &Package, host: &Host) -> Result<PathBuf> {
    let options: Options = package.options()?;
    let id = package_id(package)?;
    let version = Version::parse(&package.version)?.plain();
    let prefix = options.prefix.as_deref().unwrap_or("").trim_matches('/');
    warn_unexpressed(package, host);

    let psmdcp = hex(&Sha256::digest(format!("{id}\n{version}")))[..32].to_string();
    let authors = package.maintainer.clone().unwrap_or_default();
    let nuspec_text = nuspec(package, id, &version, &options.dependencies)?;
    let rels = relationships(id, &psmdcp);
    let properties = core_properties(id, &version, &authors, package.description.trim());

    let mut files = Vec::new();
    for entry in &package.entries {
        match &entry.data {
            Data::Ghost | Data::Dir => {}
            Data::Symlink { target } => host.warn(&format!(
                "the symlink {} -> {target} is not written: NuGet packages have no symlinks",
                entry.path
            )),
            Data::File { src, .. } => {
                if entry.config {
                    host.warn(&format!(
                        "{} is a configuration file, but NuGet has no configuration files; it is written as a plain file",
                        entry.path
                    ));
                }
                files.push((
                    member_path(options.folder, prefix, &entry.path)?,
                    src.clone(),
                    entry.mode,
                ));
            }
        }
    }
    let members: Vec<String> = files.iter().map(|(name, _, _)| name.clone()).collect();
    let types = content_types(&members);

    let (year, month, day, hour, minute, second) = civil(package.mtime);
    let time = zip::DateTime::from_date_and_time(year, month, day, hour, minute, second)
        .map_err(|_| anyhow::anyhow!("the build time is not a valid zip date"))?;
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(time);

    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let text_parts = [
        ("_rels/.rels".to_string(), rels),
        (format!("{id}.nuspec"), nuspec_text),
        ("[Content_Types].xml".to_string(), types),
        (
            format!("package/services/metadata/core-properties/{psmdcp}.psmdcp"),
            properties,
        ),
    ];
    for (name, text) in text_parts {
        writer.start_file(&name, options)?;
        writer.write_all(text.as_bytes())?;
    }
    for (name, src, mode) in files {
        writer.start_file(&name, options.unix_permissions(mode))?;
        let mut file =
            std::fs::File::open(&src).with_context(|| format!("cannot read {}", src.display()))?;
        std::io::copy(&mut file, &mut writer)?;
    }
    let bytes = writer.finish()?.into_inner();
    std::fs::create_dir_all(&package.out)?;
    let path = package.output(&file_name(package)?);
    std::fs::File::create(&path)?.write_all(&bytes)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_nightly_version_is_a_valid_semver_two_string() {
        let version = Version::parse("1.2.3-nightly.20261031.142.gabc1234").unwrap();
        assert_eq!(version.plain(), "1.2.3-nightly.20261031.142.gabc1234");
    }

    #[test]
    fn member_paths_go_below_the_folder_without_the_prefix() {
        assert_eq!(
            member_path(Folder::Tools, "", "/usr/bin/x").unwrap(),
            "tools/usr/bin/x"
        );
        assert_eq!(
            member_path(Folder::Lib, "usr", "/usr/lib/a.dll").unwrap(),
            "lib/lib/a.dll"
        );
        assert!(member_path(Folder::Tools, "usr/share", "/etc/x").is_err());
        assert!(member_path(Folder::Tools, "", "/a/../b").is_err());
    }

    #[test]
    fn xml_text_is_escaped() {
        assert_eq!(escape("a & <b> \"c\""), "a &amp; &lt;b&gt; &quot;c&quot;");
    }

    #[test]
    fn dependencies_are_grouped_by_framework() {
        let dependencies = vec![
            Dependency {
                id: "A".into(),
                version: Some("1.0.0".into()),
                target_framework: None,
            },
            Dependency {
                id: "B".into(),
                version: None,
                target_framework: Some("net8.0".into()),
            },
        ];
        let xml = dependencies_xml(&dependencies);
        assert!(xml.contains("<group>\n        <dependency id=\"A\" version=\"1.0.0\" />"));
        assert!(
            xml.contains("<group targetFramework=\"net8.0\">\n        <dependency id=\"B\" />")
        );
    }

    #[test]
    fn known_unix_times_give_their_calendar_date() {
        assert_eq!(civil(0), (1980, 1, 1, 0, 0, 0));
        assert_eq!(civil(1_700_000_000), (2023, 11, 14, 22, 13, 20));
    }
}
