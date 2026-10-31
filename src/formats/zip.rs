//! A zip archive of the files, for Windows and macOS users and for anyone who does not unpack tar files.

use crate::manifest::Manifest;
use crate::package::Package;
use crate::tree::Data;
use crate::version::Version;
use anyhow::{Context, Result};
use std::io::Write;
use std::path::PathBuf;
use zip::write::SimpleFileOptions;

/// `name-version-arch.zip`.
pub fn file_name(manifest: &Manifest) -> Result<String> {
    let version = Version::parse(&manifest.version)?.plain();
    Ok(format!("{}-{version}-{}.zip", manifest.name, manifest.arch))
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
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    } as u8;
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

pub fn build(package: &Package) -> Result<PathBuf> {
    let manifest = package.manifest;
    let version = Version::parse(&manifest.version)?.plain();
    let top = if manifest.zip.top_folder {
        format!("{}-{version}/", manifest.name)
    } else {
        String::new()
    };
    let (year, month, day, hour, minute, second) = civil(package.options.mtime);
    let time = zip::DateTime::from_date_and_time(year, month, day, hour, minute, second)
        .map_err(|_| anyhow::anyhow!("the build time is not a valid zip date"))?;
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for entry in &package.entries {
        let name = format!("{top}{}", entry.path.trim_start_matches('/'));
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(entry.mode)
            .last_modified_time(time);
        match &entry.data {
            Data::Ghost => {}
            Data::Dir => writer.add_directory(format!("{name}/"), options)?,
            Data::Symlink { target } => writer.add_symlink(&name, target, options)?,
            Data::File { src, .. } => {
                writer.start_file(&name, options)?;
                let mut file = std::fs::File::open(src)
                    .with_context(|| format!("cannot read {}", src.display()))?;
                std::io::copy(&mut file, &mut writer)?;
            }
        }
    }
    let bytes = writer.finish()?.into_inner();
    std::fs::create_dir_all(&package.options.out)?;
    let path = package.output(&file_name(manifest)?);
    std::fs::File::create(&path)?.write_all(&bytes)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::civil;

    #[test]
    fn unix_times_become_calendar_dates_and_old_ones_clamp_to_1980() {
        assert_eq!(civil(1_793_482_000), (2026, 10, 31, 21, 26, 40));
        assert_eq!(civil(951_782_400), (2000, 2, 29, 0, 0, 0));
        assert_eq!(civil(0), (1980, 1, 1, 0, 0, 0));
    }
}
