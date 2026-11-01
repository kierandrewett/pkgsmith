//! Windows Installer (MSI), written unsigned.
//!
//! An MSI is an OLE compound file that holds a relational database (the tables below), a Summary Information stream and
//! the files in one embedded cabinet. The `msi` crate writes the database, and the `cab` crate writes the cabinet.
//!
//! Specifications used:
//! - Database tables and their columns: https://learn.microsoft.com/en-us/windows/win32/msi/database-tables
//! - The action sequence in `InstallExecuteSequence`:
//!   https://learn.microsoft.com/en-us/windows/win32/msi/suggested-installexecutesequence
//! - Where `RemoveExistingProducts` may go (after `InstallFinalize`, the most efficient place) and that
//!   `FindRelatedProducts` must come before it:
//!   https://learn.microsoft.com/en-us/windows/win32/msi/removeexistingproducts-action
//! - Summary Information (Word Count 2 for a package that needs elevation, 10 for one that does not, Page Count 200 for
//!   Windows Installer 2.0): https://learn.microsoft.com/en-us/windows/win32/msi/summary-information-stream-property-set
//!
//! The layout: every folder is a component whose key path is the folder (a `CreateFolder` row makes the installer
//! create it and remove it when it is empty), and every file is a component of its own whose key path is the file.
//! Entries map below `prefix` (default `/usr`) into the install folder: `/usr/bin/hi` is `INSTALLFOLDER\bin\hi`.

use anyhow::{Context, Result, anyhow, bail};
use cab::{CabinetBuilder, CompressionType};
use msi::{CodePage, Column, Insert, LanguageId, Package as Database, PackageType, Value};
use pkgsmith_plugin::version::Version;
use pkgsmith_plugin::{Data, Host, Package};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{Cursor, Write};
use std::path::PathBuf;
use std::time::{Duration, UNIX_EPOCH};
use uuid::Uuid;

/// Namespace of the version 5 identifiers pkgsmith derives, so equal input gives equal product and component codes.
const NAMESPACE: Uuid = Uuid::from_u128(0x6f8c_4d1e_3b52_4f6a_9d07_2a1c_e5b8_7c40);
const LANGUAGE: u16 = 1033;
const CABINET: &str = "data1.cab";

/// The `msi:` section of the manifest.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    /// `machine` installs for all users into Program Files and needs elevation. `user` installs for the current user
    /// into the local application data folder. Default `machine`.
    scope: Option<String>,
    /// The folder below Program Files (or `Programs` of the user), written with `\` or `/`. Default: the package name,
    /// and `Programs\<name>` for scope `user`.
    install_dir: Option<String>,
    /// The manufacturer Windows shows. Default: the maintainer.
    manufacturer: Option<String>,
    /// The upgrade code, a GUID. Default: derived from the package name, so every version of a package shares it and a
    /// newer one replaces an older one.
    upgrade_code: Option<String>,
    /// Entries are taken relative to this absolute Unix path. Default `/usr`. A path outside it stops the build.
    prefix: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    Machine,
    User,
}

impl Options {
    fn scope(&self) -> Result<Scope> {
        match self.scope.as_deref() {
            None | Some("machine") => Ok(Scope::Machine),
            Some("user") => Ok(Scope::User),
            Some(other) => bail!("msi.scope {other:?} must be machine or user"),
        }
    }
}

/// `name-version-arch.msi`.
pub fn file_name(package: &Package) -> String {
    format!("{}-{}-{}.msi", package.name, package.version, package.arch)
}

/// A file or a folder of the installed tree, below the install folder.
#[derive(Debug)]
struct Node {
    /// Path parts below the install folder. Empty is the install folder itself.
    parts: Vec<String>,
}

struct FileItem {
    key: String,
    component: String,
    directory: String,
    name: String,
    src: PathBuf,
    size: u64,
    sequence: i32,
}

struct Folder {
    id: String,
    parent: String,
    name: String,
    component: String,
}

/// Builds the package and returns its path.
pub fn build(package: &Package, host: &Host) -> Result<PathBuf> {
    let options: Options = package.options()?;
    let scope = options.scope()?;
    let version = product_version(package, host)?;
    warn_unexpressed(package, host);

    let manufacturer = options
        .manufacturer
        .clone()
        .or_else(|| package.maintainer.clone())
        .ok_or_else(|| {
            anyhow!(
                "msi needs a manufacturer: set msi.manufacturer or the maintainer of the manifest"
            )
        })?;
    let upgrade_code = match &options.upgrade_code {
        Some(code) => Uuid::parse_str(code)
            .with_context(|| format!("msi.upgrade_code {code:?} is not a GUID"))?,
        None => Uuid::new_v5(&NAMESPACE, format!("upgrade:{}", package.name).as_bytes()),
    };
    let product_code = Uuid::new_v5(
        &NAMESPACE,
        format!(
            "product:{}:{}:{}:{}",
            package.name,
            version,
            package.arch,
            scope_text(scope)
        )
        .as_bytes(),
    );
    let package_code = Uuid::new_v5(
        &NAMESPACE,
        format!("package:{product_code}:{}", package.mtime).as_bytes(),
    );

    let root = match scope {
        Scope::User => "LocalAppDataFolder",
        Scope::Machine if package.arch == "all" => "ProgramFilesFolder",
        Scope::Machine => "ProgramFiles64Folder",
    };
    let install_dir = install_dir(&options, package, scope)?;
    let prefix = options.prefix.clone().unwrap_or_else(|| "/usr".into());
    let (folders, files) = layout(package, &prefix, &install_dir, root)?;
    if files.is_empty() {
        bail!("an MSI needs at least one file, and the manifest has none below {prefix}");
    }
    let total: u64 = files.iter().map(|f| f.size).sum();
    if total >= 1 << 31 {
        bail!("the files are {total} bytes, and an embedded cabinet holds less than 2 GiB");
    }
    if files.len() > i16::MAX as usize {
        bail!(
            "an MSI numbers its files with a 16 bit sequence, so it holds at most {} files",
            i16::MAX
        );
    }

    let cabinet = cabinet(package, &files)?;

    let mut database = Database::create(PackageType::Installer, Cursor::new(Vec::new()))?;
    database.set_database_codepage(CodePage::Windows1252);
    {
        let summary = database.summary_info_mut();
        summary.set_codepage(CodePage::Windows1252);
        summary.set_title("Installation Database");
        summary.set_subject(ascii(&package.name));
        summary.set_author(ascii(&manufacturer));
        summary.set_comments(ascii(package.description.lines().next().unwrap_or("")));
        summary.set_creating_application("pkgsmith");
        summary.set_arch(match package.arch.as_str() {
            "amd64" => "x64",
            "arm64" => "Arm64",
            _ => "Intel",
        });
        summary.set_languages(&[LanguageId::from_id(LANGUAGE)]);
        summary.set_uuid(package_code);
        summary.set_page_count(200);
        summary.set_word_count(match scope {
            Scope::Machine => 2,
            Scope::User => 10,
        });
        summary.set_creation_time(UNIX_EPOCH + Duration::from_secs(package.mtime));
    }
    let tables = Tables {
        package,
        scope,
        product_code,
        upgrade_code,
        manufacturer: &manufacturer,
        version: &version,
        folders: &folders,
        files: &files,
        root,
    };
    tables.write(&mut database)?;
    {
        let mut stream = database.write_stream(CABINET)?;
        stream.write_all(&cabinet)?;
    }
    database.flush()?;
    let bytes = database.into_inner()?.into_inner();

    let path = package.out.join(file_name(package));
    let mut out =
        File::create(&path).with_context(|| format!("cannot write {}", path.display()))?;
    out.write_all(&bytes)?;
    Ok(path)
}

fn scope_text(scope: Scope) -> &'static str {
    match scope {
        Scope::Machine => "machine",
        Scope::User => "user",
    }
}

/// An MSI product version is `major.minor.build`: 8 bits, 8 bits and 16 bits.
fn product_version(package: &Package, host: &Host) -> Result<String> {
    let version = Version::parse(&package.version)?;
    if version.pre.is_some() {
        host.warn(&format!(
            "the pre-release text of {:?} is not kept: an MSI version is three numbers",
            package.version
        ));
    }
    if package.release != "1" && !package.release.is_empty() {
        host.warn("the release is not part of an MSI product version, so it is not written");
    }
    let parts: Vec<u32> = version
        .core
        .split('.')
        .map(|n| {
            n.parse()
                .context("the version has a part that is not a number")
        })
        .collect::<Result<_>>()?;
    if parts.len() != 3 || parts[0] > 255 || parts[1] > 255 || parts[2] > 65535 {
        bail!(
            "the version {} does not fit an MSI product version: the parts are at most 255, 255 and 65535",
            version.core
        );
    }
    Ok(version.core)
}

fn install_dir(options: &Options, package: &Package, scope: Scope) -> Result<Vec<String>> {
    let text = match (&options.install_dir, scope) {
        (Some(dir), _) => dir.clone(),
        (None, Scope::Machine) => package.name.clone(),
        (None, Scope::User) => format!("Programs\\{}", package.name),
    };
    let mut parts = Vec::new();
    for part in text.split(['\\', '/']).filter(|p| !p.is_empty()) {
        check_name(part, "msi.install_dir")?;
        parts.push(part.to_owned());
    }
    if parts.is_empty() {
        bail!("msi.install_dir is empty");
    }
    Ok(parts)
}

/// A name that Windows and the installer database accept: ASCII, and none of the characters Windows refuses.
fn check_name(name: &str, what: &str) -> Result<()> {
    if name.is_empty() || name == "." || name == ".." {
        bail!("{what}: {name:?} is not a file name");
    }
    if !name.is_ascii() {
        bail!(
            "{what}: {name:?} is not ASCII; the installer database holds ASCII names here, so pkgsmith refuses others"
        );
    }
    if let Some(c) = name
        .chars()
        .find(|c| c.is_control() || "<>:\"/\\|?*".contains(*c))
    {
        bail!("{what}: {name:?} has the character {c:?}, which Windows does not allow in a name");
    }
    if name.ends_with('.') || name.ends_with(' ') {
        bail!("{what}: {name:?} ends with a dot or a space, which Windows does not allow");
    }
    Ok(())
}

/// Text for a database column: ASCII only, so it survives the Windows-1252 code page of the database.
fn ascii(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii() && !c.is_control() {
                c
            } else {
                '?'
            }
        })
        .collect()
}

/// The 8.3 name of `long`, unique among `taken` (upper case names already used in the same folder).
fn short_name(long: &str, taken: &mut BTreeSet<String>) -> String {
    let upper = long.to_ascii_uppercase();
    let (base, ext) = match upper.rsplit_once('.') {
        Some((base, ext)) if !base.is_empty() => (base.to_owned(), ext.to_owned()),
        _ => (upper.clone(), String::new()),
    };
    let allowed = |c: char| c.is_ascii_alphanumeric() || "_$~!#%&'()-@^`{}".contains(c);
    let fits = base.len() <= 8
        && ext.len() <= 3
        && base.chars().all(allowed)
        && ext.chars().all(allowed)
        && !base.contains('.');
    if fits {
        return long.to_owned();
    }
    let clean = |text: &str, limit: usize| -> String {
        text.chars().filter(|c| allowed(*c)).take(limit).collect()
    };
    let base = clean(&base.replace('.', ""), 8);
    let base = if base.is_empty() {
        "FILE".to_owned()
    } else {
        base
    };
    let ext = clean(&ext, 3);
    for n in 1.. {
        let tail = format!("~{n}");
        let stem: String = base.chars().take(8 - tail.len()).collect();
        let name = if ext.is_empty() {
            format!("{stem}{tail}")
        } else {
            format!("{stem}{tail}.{ext}")
        };
        if taken.insert(name.clone()) {
            return name;
        }
    }
    unreachable!("some ~N is free")
}

/// `SHORT|long`, or just the name when it already is a valid 8.3 name.
fn db_name(long: &str, taken: &mut BTreeSet<String>) -> String {
    let short = short_name(long, taken);
    if short == long {
        long.to_owned()
    } else {
        format!("{short}|{long}")
    }
}

/// The folders and files below the install folder, with identifiers and database names.
fn layout(
    package: &Package,
    prefix: &str,
    install_dir: &[String],
    root: &str,
) -> Result<(Vec<Folder>, Vec<FileItem>)> {
    let prefix = prefix.trim_end_matches('/');
    if !prefix.is_empty() && !prefix.starts_with('/') {
        bail!("msi.prefix {prefix:?} must be an absolute path");
    }
    let mut dirs: BTreeSet<Vec<String>> = BTreeSet::new();
    let mut file_nodes: Vec<(Node, &pkgsmith_plugin::Entry)> = Vec::new();
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    for entry in &package.entries {
        let rest = match entry.path.strip_prefix(prefix) {
            Some(rest) if rest.is_empty() || rest.starts_with('/') => rest,
            _ => bail!(
                "{} is outside the prefix {prefix:?}; move it below the prefix or change msi.prefix",
                entry.path
            ),
        };
        let parts: Vec<String> = rest
            .split('/')
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
            .collect();
        for part in &parts {
            check_name(part, &entry.path)?;
        }
        let key = parts.join("\\").to_ascii_lowercase();
        match &entry.data {
            Data::File { .. } => {
                if parts.is_empty() {
                    bail!("{} cannot be the install folder itself", entry.path);
                }
                if let Some(other) = seen.insert(key, entry.path.clone()) {
                    bail!(
                        "{} and {other} differ only in case, and Windows names ignore case",
                        entry.path
                    );
                }
                for n in 1..parts.len() {
                    dirs.insert(parts[..n].to_vec());
                }
                file_nodes.push((Node { parts }, entry));
            }
            Data::Dir => {
                for n in 1..=parts.len() {
                    dirs.insert(parts[..n].to_vec());
                }
            }
            Data::Symlink { .. } | Data::Ghost => {}
        }
    }

    // The install folder and the folders above it, then every folder of the tree.
    let mut folders: Vec<Folder> = Vec::new();
    let mut ids: BTreeMap<Vec<String>, String> = BTreeMap::new();
    let mut taken: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut parent = root.to_owned();
    for (n, part) in install_dir.iter().enumerate() {
        let id = if n + 1 == install_dir.len() {
            "INSTALLFOLDER".to_owned()
        } else {
            format!("INSTALLPARENT{}", n + 1)
        };
        let name = db_name(part, taken.entry(parent.clone()).or_default());
        folders.push(Folder {
            id: id.clone(),
            parent: parent.clone(),
            name,
            component: format!("Cd_{id}"),
        });
        parent = id;
    }
    let mut next = 0u32;
    for dir in &dirs {
        next += 1;
        let id = format!("D{next}");
        let up = if dir.len() == 1 {
            "INSTALLFOLDER".to_owned()
        } else {
            ids[&dir[..dir.len() - 1]].clone()
        };
        let name = db_name(
            dir.last().expect("a folder has a name"),
            taken.entry(up.clone()).or_default(),
        );
        folders.push(Folder {
            id: id.clone(),
            parent: up,
            name,
            component: format!("Cd_{id}"),
        });
        ids.insert(dir.clone(), id);
    }

    let mut files: Vec<FileItem> = Vec::new();
    for (index, (node, entry)) in file_nodes.iter().enumerate() {
        let Data::File { src, size } = &entry.data else {
            continue;
        };
        let name = node.parts.last().expect("a file has a name").clone();
        let directory = if node.parts.len() == 1 {
            "INSTALLFOLDER".to_owned()
        } else {
            ids[&node.parts[..node.parts.len() - 1]].clone()
        };
        let short = db_name(&name, taken.entry(directory.clone()).or_default());
        files.push(FileItem {
            key: format!("f{}", index + 1),
            component: format!("Cf_{}", index + 1),
            directory,
            name: short,
            src: src.clone(),
            size: *size,
            sequence: 0,
        });
    }
    // Sequence follows the order of the files in the cabinet.
    for (n, file) in files.iter_mut().enumerate() {
        file.sequence = (n + 1) as i32;
    }
    Ok((folders, files))
}

fn cabinet(package: &Package, files: &[FileItem]) -> Result<Vec<u8>> {
    let stamp = time::OffsetDateTime::from_unix_timestamp(package.mtime.max(315_532_800) as i64)
        .context("the time stamp does not fit a cabinet")?;
    let stamp = time::PrimitiveDateTime::new(stamp.date(), stamp.time());
    let mut builder = CabinetBuilder::new();
    {
        let folder = builder.add_folder(CompressionType::MsZip);
        for file in files {
            folder.add_file(file.key.clone()).set_datetime(stamp);
        }
    }
    let mut writer = builder.build(Cursor::new(Vec::new()))?;
    let mut by_name = files.iter();
    while let Some(mut target) = writer.next_file()? {
        let file = by_name
            .next()
            .ok_or_else(|| anyhow!("the cabinet asks for more files than the package has"))?;
        let mut source =
            File::open(&file.src).with_context(|| format!("cannot read {}", file.src.display()))?;
        std::io::copy(&mut source, &mut target)
            .with_context(|| format!("cannot add {} to the cabinet", file.src.display()))?;
    }
    Ok(writer.finish()?.into_inner())
}

struct Tables<'a> {
    package: &'a Package,
    scope: Scope,
    product_code: Uuid,
    upgrade_code: Uuid,
    manufacturer: &'a str,
    version: &'a str,
    folders: &'a [Folder],
    files: &'a [FileItem],
    root: &'a str,
}

fn text(value: impl Into<String>) -> Value {
    Value::Str(value.into())
}

fn guid(uuid: Uuid) -> String {
    format!("{{{}}}", uuid.to_string().to_ascii_uppercase())
}

impl Tables<'_> {
    fn component_guid(&self, id: &str) -> String {
        guid(Uuid::new_v5(
            &NAMESPACE,
            format!("component:{}:{id}", self.product_code).as_bytes(),
        ))
    }

    fn write(&self, db: &mut Database<Cursor<Vec<u8>>>) -> Result<()> {
        let sixty_four = self.package.arch == "amd64" || self.package.arch == "arm64";
        let component_attributes: i32 = if sixty_four && self.scope == Scope::Machine {
            256
        } else {
            0
        };

        db.create_table(
            "Property",
            vec![
                Column::build("Property").primary_key().id_string(72),
                Column::build("Value").localizable().text_string(0),
            ],
        )?;
        let mut properties: Vec<(&str, String)> = vec![
            ("ProductCode", guid(self.product_code)),
            ("ProductName", ascii(&self.package.name)),
            ("ProductVersion", self.version.to_owned()),
            ("Manufacturer", ascii(self.manufacturer)),
            ("ProductLanguage", LANGUAGE.to_string()),
            ("UpgradeCode", guid(self.upgrade_code)),
            ("SecureCustomProperties", "PREVIOUSVERSIONSINSTALLED".into()),
            ("ARPNOMODIFY", "1".into()),
        ];
        if self.scope == Scope::Machine {
            properties.push(("ALLUSERS", "1".into()));
        } else {
            properties.push(("MSIINSTALLPERUSER", "1".into()));
        }
        if let Some(homepage) = &self.package.homepage {
            properties.push(("ARPURLINFOABOUT", ascii(homepage)));
        }
        if let Some(summary) = self.package.description.lines().next()
            && !summary.is_empty()
        {
            properties.push(("ARPCOMMENTS", ascii(summary)));
        }
        db.insert_rows(
            Insert::into("Property").rows(
                properties
                    .into_iter()
                    .map(|(k, v)| vec![text(k), text(v)])
                    .collect(),
            ),
        )?;

        db.create_table(
            "Directory",
            vec![
                Column::build("Directory").primary_key().id_string(72),
                Column::build("Directory_Parent").nullable().id_string(72),
                Column::build("DefaultDir").localizable().text_string(255),
            ],
        )?;
        let mut directories = vec![
            vec![text("TARGETDIR"), Value::Null, text("SourceDir")],
            vec![text(self.root), text("TARGETDIR"), text(".")],
        ];
        for folder in self.folders {
            directories.push(vec![
                text(&folder.id),
                text(&folder.parent),
                text(&folder.name),
            ]);
        }
        db.insert_rows(Insert::into("Directory").rows(directories))?;

        db.create_table(
            "Component",
            vec![
                Column::build("Component").primary_key().id_string(72),
                Column::build("ComponentId").nullable().string(38),
                Column::build("Directory_").id_string(72),
                Column::build("Attributes").int16(),
                Column::build("Condition").nullable().string(255),
                Column::build("KeyPath").nullable().id_string(72),
            ],
        )?;
        let mut components: Vec<Vec<Value>> = Vec::new();
        for folder in self.folders {
            components.push(vec![
                text(&folder.component),
                text(self.component_guid(&folder.component)),
                text(&folder.id),
                Value::Int(component_attributes),
                Value::Null,
                Value::Null,
            ]);
        }
        for file in self.files {
            components.push(vec![
                text(&file.component),
                text(self.component_guid(&file.component)),
                text(&file.directory),
                Value::Int(component_attributes),
                Value::Null,
                text(&file.key),
            ]);
        }
        db.insert_rows(Insert::into("Component").rows(components))?;

        db.create_table(
            "Feature",
            vec![
                Column::build("Feature").primary_key().id_string(38),
                Column::build("Feature_Parent").nullable().id_string(38),
                Column::build("Title")
                    .nullable()
                    .localizable()
                    .text_string(64),
                Column::build("Description")
                    .nullable()
                    .localizable()
                    .text_string(255),
                Column::build("Display").nullable().int16(),
                Column::build("Level").int16(),
                Column::build("Directory_").nullable().id_string(72),
                Column::build("Attributes").int16(),
            ],
        )?;
        db.insert_rows(Insert::into("Feature").row(vec![
            text("Main"),
            Value::Null,
            text(ascii(&self.package.name)),
            Value::Null,
            Value::Int(1),
            Value::Int(1),
            text("INSTALLFOLDER"),
            Value::Int(0),
        ]))?;

        db.create_table(
            "FeatureComponents",
            vec![
                Column::build("Feature_").primary_key().id_string(38),
                Column::build("Component_").primary_key().id_string(72),
            ],
        )?;
        let mut links: Vec<Vec<Value>> = Vec::new();
        for folder in self.folders {
            links.push(vec![text("Main"), text(&folder.component)]);
        }
        for file in self.files {
            links.push(vec![text("Main"), text(&file.component)]);
        }
        db.insert_rows(Insert::into("FeatureComponents").rows(links))?;

        db.create_table(
            "CreateFolder",
            vec![
                Column::build("Directory_").primary_key().id_string(72),
                Column::build("Component_").primary_key().id_string(72),
            ],
        )?;
        db.insert_rows(
            Insert::into("CreateFolder").rows(
                self.folders
                    .iter()
                    .map(|f| vec![text(&f.id), text(&f.component)])
                    .collect(),
            ),
        )?;

        db.create_table(
            "File",
            vec![
                Column::build("File").primary_key().id_string(72),
                Column::build("Component_").id_string(72),
                Column::build("FileName").localizable().text_string(255),
                Column::build("FileSize").int32(),
                Column::build("Version").nullable().string(72),
                Column::build("Language").nullable().string(20),
                Column::build("Attributes").nullable().int16(),
                Column::build("Sequence").int16(),
            ],
        )?;
        db.insert_rows(
            Insert::into("File").rows(
                self.files
                    .iter()
                    .map(|f| {
                        vec![
                            text(&f.key),
                            text(&f.component),
                            text(&f.name),
                            Value::Int(f.size as i32),
                            Value::Null,
                            Value::Null,
                            Value::Int(0),
                            Value::Int(f.sequence),
                        ]
                    })
                    .collect(),
            ),
        )?;

        db.create_table(
            "Media",
            vec![
                Column::build("DiskId").primary_key().int16(),
                Column::build("LastSequence").int16(),
                Column::build("DiskPrompt")
                    .nullable()
                    .localizable()
                    .text_string(64),
                Column::build("Cabinet").nullable().string(255),
                Column::build("VolumeLabel").nullable().text_string(32),
                Column::build("Source").nullable().string(32),
            ],
        )?;
        db.insert_rows(Insert::into("Media").row(vec![
            Value::Int(1),
            Value::Int(self.files.len() as i32),
            Value::Null,
            text(format!("#{CABINET}")),
            Value::Null,
            Value::Null,
        ]))?;

        db.create_table(
            "Upgrade",
            vec![
                Column::build("UpgradeCode").primary_key().string(38),
                Column::build("VersionMin")
                    .primary_key()
                    .nullable()
                    .string(20),
                Column::build("VersionMax")
                    .primary_key()
                    .nullable()
                    .string(20),
                Column::build("Language")
                    .primary_key()
                    .nullable()
                    .string(255),
                Column::build("Attributes").primary_key().int32(),
                Column::build("Remove").nullable().string(255),
                Column::build("ActionProperty").id_string(72),
            ],
        )?;
        db.insert_rows(Insert::into("Upgrade").row(vec![
            text(guid(self.upgrade_code)),
            Value::Null,
            text(self.version),
            Value::Null,
            Value::Int(0),
            Value::Null,
            text("PREVIOUSVERSIONSINSTALLED"),
        ]))?;

        db.create_table(
            "InstallExecuteSequence",
            vec![
                Column::build("Action").primary_key().id_string(72),
                Column::build("Condition").nullable().string(255),
                Column::build("Sequence").nullable().int16(),
            ],
        )?;
        // The numbers are the suggested sequence of the Windows Installer documentation. FindRelatedProducts only has
        // to come before RemoveExistingProducts, which goes after InstallFinalize, the most efficient place.
        let sequence: [(&str, i32); 18] = [
            ("FindRelatedProducts", 25),
            ("CostInitialize", 800),
            ("FileCost", 900),
            ("CostFinalize", 1000),
            ("InstallValidate", 1400),
            ("InstallInitialize", 1500),
            ("ProcessComponents", 1600),
            ("UnpublishFeatures", 1800),
            ("RemoveFiles", 3500),
            ("RemoveFolders", 3600),
            ("CreateFolders", 3700),
            ("InstallFiles", 4000),
            ("RegisterUser", 6000),
            ("RegisterProduct", 6100),
            ("PublishFeatures", 6300),
            ("PublishProduct", 6400),
            ("InstallFinalize", 6600),
            ("RemoveExistingProducts", 6700),
        ];
        db.insert_rows(
            Insert::into("InstallExecuteSequence").rows(
                sequence
                    .iter()
                    .map(|(action, number)| vec![text(*action), Value::Null, Value::Int(*number)])
                    .collect(),
            ),
        )?;
        Ok(())
    }
}

fn warn_unexpressed(package: &Package, host: &Host) {
    let lists = [
        ("depends", &package.depends),
        ("recommends", &package.recommends),
        ("suggests", &package.suggests),
        ("conflicts", &package.conflicts),
        ("replaces", &package.replaces),
        ("provides", &package.provides),
    ];
    for (field, values) in lists {
        if !values.is_empty() {
            host.warn(&format!(
                "MSI has no {field} field, so {} is not written",
                values.join(", ")
            ));
        }
    }
    let scripts = &package.scripts;
    if [
        &scripts.pre_install,
        &scripts.post_install,
        &scripts.pre_remove,
        &scripts.post_remove,
    ]
    .iter()
    .any(|script| script.is_some())
    {
        host.warn("MSI runs no shell scripts, so the package scripts are not written");
    }
    if package.license.is_some() {
        host.warn("MSI has no license field here, so the license is not written");
    }
    let mut ghosts = 0;
    let mut links = 0;
    let mut configs = 0;
    let mut owned = false;
    for entry in &package.entries {
        match entry.data {
            Data::Ghost => ghosts += 1,
            Data::Symlink { .. } => links += 1,
            Data::File { .. } | Data::Dir => {}
        }
        if entry.config {
            configs += 1;
        }
        let standard_mode = matches!(entry.mode & 0o7777, 0o644 | 0o755);
        if entry.owner != "root" || entry.group != "root" || !standard_mode {
            owned = true;
        }
    }
    if ghosts > 0 {
        host.warn(&format!(
            "{ghosts} ghost entries are owned by no package, so they are not written"
        ));
    }
    if links > 0 {
        host.warn(&format!(
            "{links} symbolic links are not written: Windows Installer cannot create them"
        ));
    }
    if configs > 0 {
        host.warn(&format!(
            "{configs} configuration files are installed as ordinary files: an MSI replaces them on an upgrade"
        ));
    }
    if owned {
        host.warn("owner, group and mode are not kept: Windows sets the access rules of the installed files");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_eight_dot_three_names_stay_and_others_get_a_unique_short_name() {
        let mut taken = BTreeSet::new();
        assert_eq!(db_name("hi.txt", &mut taken), "hi.txt");
        let first = db_name("with space.txt", &mut taken);
        let second = db_name("with space.dat", &mut taken);
        assert_eq!(first, "WITHSP~1.TXT|with space.txt");
        assert_eq!(second, "WITHSP~1.DAT|with space.dat");
        let third = db_name("with spaces.txt", &mut taken);
        assert_eq!(third, "WITHSP~2.TXT|with spaces.txt");
    }

    #[test]
    fn names_windows_refuses_are_rejected() {
        for bad in ["a:b", "a|b", "a?", "trailing.", "caf\u{e9}", ".."] {
            assert!(check_name(bad, "test").is_err(), "{bad:?}");
        }
        assert!(check_name("fine name.txt", "test").is_ok());
    }
}
